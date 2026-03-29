// ============================================================
//  src/util/hexdump.rs  —  字节序列漂亮打印
// ============================================================

pub fn hexdump(label: &str, bytes: &[u8]) {
    let width = label.len() + 22;
    println!("\n╔══ {} ({} bytes) ══╗", label, bytes.len());
    for (i, chunk) in bytes.chunks(16).enumerate() {
        print!("  {:04x}  ", i * 16);
        for b in chunk {
            print!("{:02x} ", b);
        }
        for _ in 0..(16 - chunk.len()) {
            print!("   ");
        }
        print!(" │");
        for b in chunk {
            let c = if b.is_ascii_graphic() || *b == b' ' { *b as char } else { '.' };
            print!("{}", c);
        }
        println!("│");
    }
    println!("╚{:═<width$}╝", "", width = width);
}
