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

pub fn hex_disassemble(name: &str, code: &[u8]) {
    println!("\n┏━━━ Disassembly: {} ({} bytes) ━━━┓", name, code.len());
    
    // 打印架构信息，方便对比
    #[cfg(target_arch = "x86_64")]
    println!("┃ Target: x86-64 (CISC)            ┃");
    #[cfg(target_arch = "aarch64")]
    println!("┃ Target: AArch64 (RISC)           ┃");
    #[cfg(target_arch = "arm")]
    println!("┃ Target: ARM (Thumb-2/A32)        ┃");
    
    println!("┣━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┫");

    for (i, chunk) in code.chunks(16).enumerate() {
        print!("┃ {:04x}: ", i * 16);
        for b in chunk {
            print!("{:02x} ", b);
        }
        // 对齐补白
        if chunk.len() < 16 {
            for _ in 0..(16 - chunk.len()) { print!("   "); }
        }
        
        // 打印简易字符预览（ASCII）
        print!(" │ ");
        for b in chunk {
            let c = if *b >= 32 && *b <= 126 { *b as char } else { '.' };
            print!("{}", c);
        }
        println!(" ┃");
    }
    println!("┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛");
}