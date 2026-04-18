# Process Write Memory
WriteProcessMemory ile veri yazma ve ReadProcessMemory ile yazılan veriyi doğrulama işlemlerini gerçekleştirmek.

## Özellikler
- Hedef süreci isme göre tarar ve geniş erişim yetkileriyle (VM_OPERATION, VM_WRITE, VM_READ) açar.
- `VirtualAllocEx` ile uzak süreçte 4096 byte büyüklüğünde sayfa ayırır.
- `WriteProcessMemory` kullanarak ayrılan alana 8 byte'lık veri bloğu yazar.
- `ReadProcessMemory` ile yazılan veriyi bellekten geri okuyarak doğrulama yapar.
- Kaynak yönetimi prensiplerine uygun olarak süreç sonunda ayrılan belleği serbest bırakır.

## Teknik Detaylar
- WinAPI etkileşimi `windows-sys` kütüphanesi ile sağlanmıştır.
- `VirtualQueryEx` ile bellek durumunun (MEM_COMMIT) ve izinlerinin (PAGE_READWRITE) kontrolü yapılır.
- Yazma işlemi sonrası byte seviyesinde tutarlılık kontrolü gerçekleştirilir.

```bash
cargo run
