use core::ptr;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Memory::{
    VirtualAllocEx, VirtualFreeEx, VirtualQueryEx, MEMORY_BASIC_INFORMATION, MEM_COMMIT,
    MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
};

// --- LOGGING ---
macro_rules! log_info { ($($arg:tt)*) => { println!("[INFO] {}", format_args!($($arg)*)); }; }
macro_rules! log_success { ($($arg:tt)*) => { println!("[+SUCCESS] {}", format_args!($($arg)*)); }; }
macro_rules! log_error { 
    ($msg:expr) => { eprintln!("[!ERROR] {} (Win32 Error: {})", $msg, unsafe { GetLastError() }); };
    ($msg:expr, $err:expr) => { eprintln!("[!ERROR] {} (Win32 Error: {})", $msg, $err); };
}

// --- RAII WRAPPERS ---

struct SnapshotHandle(isize);
impl Drop for SnapshotHandle {
    fn drop(&mut self) {
        if self.0 != INVALID_HANDLE_VALUE {
            unsafe { CloseHandle(self.0) };
        }
    }
}

struct ProcessHandle(isize);
impl Drop for ProcessHandle {
    fn drop(&mut self) {
        if self.0 != 0 {
            unsafe { CloseHandle(self.0) };
            log_info!("Process handle closed.");
        }
    }
}

struct RemoteMemory {
    process_handle: isize,
    address: *mut core::ffi::c_void,
}
impl Drop for RemoteMemory {
    fn drop(&mut self) {
        if !self.address.is_null() {
            let status = unsafe { VirtualFreeEx(self.process_handle, self.address, 0, MEM_RELEASE) };
            if status != 0 {
                log_success!("Remote memory released automatically via RAII.");
            } else {
                log_error!("RAII Cleanup failed for remote memory");
            }
        }
    }
}

// --- CORE LOGIC ---

fn get_process_id(name: &str) -> Result<u32, String> {
    unsafe {
        let snapshot = SnapshotHandle(CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0));
        if snapshot.0 == INVALID_HANDLE_VALUE {
            return Err(format!("Snapshot failed. Win32 Error: {}", GetLastError()));
        }

        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

        if Process32FirstW(snapshot.0, &mut entry) != 0 {
            loop {
                let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
                
                // FIX: .into_owned() ekleyerek veriyi kopyalıyoruz ve sahipleniyoruz
                let pname = OsString::from_wide(&entry.szExeFile[..len]).to_string_lossy().into_owned();
                
                if pname.eq_ignore_ascii_case(name) {
                    return Ok(entry.th32ProcessID);
                }
                if Process32NextW(snapshot.0, &mut entry) == 0 { break; }
            }
        }
        Err(format!("Process '{}' not found.", name))
    }
}

fn verify_remote_allocation(handle: isize, addr: *mut core::ffi::c_void, size: usize) -> bool {
    let mut mbi: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
    let result = unsafe { VirtualQueryEx(handle, addr, &mut mbi, std::mem::size_of::<MEMORY_BASIC_INFORMATION>()) };
    
    if result == 0 {
        log_error!("Query failed");
        return false;
    }

    let valid = (mbi.State & MEM_COMMIT) != 0 && (mbi.Protect & PAGE_READWRITE) != 0 && mbi.RegionSize >= size;
    log_info!("Deep Validation -> Commit: {}, RW: {}, SizeOK: {}", (mbi.State & MEM_COMMIT) != 0, (mbi.Protect & PAGE_READWRITE) != 0, mbi.RegionSize >= size);
    valid
}

fn main() {
    let target = "test.exe";
    let size = 4096;
    let payload: [u8; 8] = [0x52, 0x55, 0x53, 0x54, 0x43, 0x4F, 0x44, 0x45];

    log_info!("Diagnostic sequence initiated for: {}", target);

    let pid = match get_process_id(target) {
        Ok(id) => { log_success!("PID found: {}", id); id },
        Err(e) => { eprintln!("[!] {}", e); return; }
    };

    let handle = ProcessHandle(unsafe {
        OpenProcess(PROCESS_VM_OPERATION | PROCESS_VM_WRITE | PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, 0, pid)
    });

    if handle.0 == 0 {
        log_error!("OpenProcess handle acquisition failed");
        return;
    }

    let remote_addr = unsafe { VirtualAllocEx(handle.0, ptr::null(), size, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE) };
    if remote_addr.is_null() {
        log_error!("VirtualAllocEx failed");
        return;
    }

    // Encapsulate address in RAII wrapper for automatic cleanup
    let remote_mem = RemoteMemory { process_handle: handle.0, address: remote_addr };
    log_success!("Memory allocated at: {:p}", remote_mem.address);

    if verify_remote_allocation(handle.0, remote_mem.address, size) {
        let mut written = 0;
        let w_status = unsafe { WriteProcessMemory(handle.0, remote_mem.address, payload.as_ptr() as _, payload.len(), &mut written) };

        if w_status != 0 && written == payload.len() {
            log_success!("Write success: {} bytes deployed.", written);

            let mut read_buf = [0u8; 8];
            let mut read_len = 0;
            let r_status = unsafe { ReadProcessMemory(handle.0, remote_mem.address, read_buf.as_mut_ptr() as _, read_buf.len(), &mut read_len) };

            if r_status != 0 && read_len == payload.len() && read_buf == payload {
                log_success!("Full verification: Memory content and byte count match.");
            } else {
                log_error!("Verification failed: Data mismatch or partial read.");
            }
        } else {
            log_error!("Write failed or partial.");
        }
    }

    log_info!("Exiting scope. RAII cleanup triggered...");
}