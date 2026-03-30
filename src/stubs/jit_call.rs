use crate::macro_asm::{MacroAssembler, MacroAssemblerBackend, NativeMasm, VReg, X64Backend};

/// 目标 Callee：将三个参数相加
extern "C" fn rust_callee_sum(a: i64, b: i64, c: i64) -> i64 {
    println!("[Callee] Received: {}, {}, {}", a, b, c);
    a + b + c
}

#[test]
fn test_emit_caller_with_sp() {
    let mut m = NativeMasm::new();
    let callee_addr = rust_callee_sum as *const () as i64;

    m.prologue(); 

    // 1. 设置参数
    m.mov_imm(VReg::Arg(0), 1);
    m.mov_imm(VReg::Arg(1), 2);
    m.mov_imm(VReg::Arg(2), 3);

    // 2. 执行调用
    let target = VReg::Tmp(0);
    m.mov_imm(target, callee_addr);
    
    m.safe_call(target, 3);


    m.add_imm(VReg::Ret, 5);
    m.epilogue();
    m.ret();

    // --- 编译并运行 ---
    unsafe {
        let jit_fn = m.compile::<unsafe extern "C" fn() -> i64>();
        let result = (jit_fn.get())();
        
        println!("[JIT] Final Result: {}", result);
        // 验证：(1 + 2 + 3) + 5 = 11
        assert_eq!(result, 11);
    }
}