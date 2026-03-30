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

    // 2. 准备调用环境 (修正版)
    #[cfg(target_os = "windows")]
    {
        // 影子空间 32 字节 + 8 字节对齐填充 = 40 字节
        // 这样：(当前 0) - 40 - (call 压栈 8) = 结束位 0 (对齐！)
        m.sub_imm(VReg::StackPtr, 40); 
    }

    // 3. 执行调用
    let tmp = VReg::Tmp(0);
    m.mov_imm(tmp, callee_addr);
    m.call_reg(tmp);

    // 4. 清理环境
    #[cfg(target_os = "windows")]
    {
        m.add_imm(VReg::StackPtr, 40);
    }

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