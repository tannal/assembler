use crate::macro_asm::{MacroAssembler, MacroAssemblerBackend, NativeMasm, VReg, X64Backend};

/// 必须使用 extern "C" 以确保 Rust 遵循标准 ABI
extern "C" fn rust_expensive_sum(
    a: i64,
    b: i64,
    c: i64,
    d: i64,
    e: i64,
    f: i64,
    g: i64,
    h: i64,
) -> i64 {
    println!(
        "[Rust] 收到 8 个参数: {}, {}, {}, {}, {}, {}, {}, {}",
        a, b, c, d, e, f, g, h
    );
    // 返回所有参数之和
    a + b + c + d + e + f + g + h
}

#[test]
fn test_call_with_8_arguments() {
    let mut m = NativeMasm::new();
    let callee_addr = rust_expensive_sum as *const () as i64;

    m.prologue();

    // --- 关键修改：手动指定已映射的 VReg ---
    // 我们只有 Tmp(0..3), Arg(0..3), Ret, Cnt, Ptr 可用
    let input_vregs = [
        VReg::Arg(0),
        VReg::Arg(1),
        VReg::Arg(2),
        VReg::Arg(3),
        VReg::Tmp(0),
        VReg::Tmp(1),
        VReg::Tmp(2),
        VReg::Tmp(3),
    ];

    // 1. 先准备 8 个参数的值
    for (i, &vreg) in input_vregs.iter().enumerate() {
        m.mov_imm(vreg, (i + 1) as i64);
    }

    // 2. 最后加载函数地址到 Cnt
    let target_reg = VReg::Cnt;
    m.mov_imm(target_reg, callee_addr);

    // 3. 调用
    m.call_with_args(target_reg, &input_vregs);

    m.epilogue();
    m.ret();

    // --- 编译并运行 ---
    unsafe {
        let jit_fn = m.compile::<unsafe extern "C" fn() -> i64>();
        let result = (jit_fn.get())();
        assert_eq!(result, 36);
    }
}
