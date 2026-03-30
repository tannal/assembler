use crate::macro_asm::{MacroAssembler, MacroAssemblerBackend, VReg, X64Backend};

// 定义一个普通的 Rust 函数
pub extern "C" fn rust_add_five(input: i64) -> i64 {
    println!("[Rust] 收到输入: {}", input);
    input + 5
}

fn emit_call_rust<B: MacroAssemblerBackend>(
    masm: &mut MacroAssembler<B>, 
    func_ptr: *const ()
) {
    // 1. 函数进入
    masm.prologue();

    // 2. 准备参数 (第一个参数用 Arg0)
    // 根据你的映射，Win: RCX, Linux: RDI
    masm.mov_imm(VReg::Arg(0), 100);

    // 3. 将 Rust 函数地址加载到临时寄存器
    let target = VReg::Tmp(0);
    masm.mov_imm(target, func_ptr as i64);

    // 4. 执行工业级安全调用
    // 参数说明：target 是地址，1 是参数个数
    masm.safe_call(target, 1);

    // 5. 结果处理：Rust 的返回值此时就在 VReg::Ret (RAX) 中
    // 我们可以在这里再做点什么，比如 +1
    masm.add_imm(VReg::Ret, 1);

    // 6. 退出
    masm.epilogue();
    masm.ret();
}

#[test]
fn test_call_native_rust() {
    let mut masm = MacroAssembler::<X64Backend>::new();
    
    // 获取函数指针
    let ptr = rust_add_five as *const ();

    // 发射代码
    emit_call_rust(&mut masm, ptr);

    // 编译
    let jit_fn = unsafe { 
        masm.compile::<unsafe extern "C" fn() -> i64>() 
    };

    // 执行
    let result = unsafe { (jit_fn.get())() };

    println!("[JIT] 最终结果: {}", result);
    // 逻辑：100 (input) + 5 (rust_add_five) + 1 (add_imm) = 106
    assert_eq!(result, 106);
}