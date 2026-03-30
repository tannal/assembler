// ============================================================
//  src/stubs/mod.rs  —  跨平台 Stub 生成器
//
//  所有 emit_xxx 函数体内部不含任何 #[cfg]。
//  架构差异完全由 MacroAssembler<NativeBackend> 抹平。
// ============================================================

use crate::macro_asm::{Cond, MacroAssembler, MacroAssemblerBackend, NativeBackend, VReg};
use crate::runtime::JitFn;
use crate::util::hexdump::hex_disassemble;

pub mod quicksort;
pub mod bubble_sort;
pub mod fib;
pub mod jit_call;

pub use quicksort::build_quicksort;
pub use bubble_sort::build_bubblesort;
pub use fib::build_fibonacci;

// ──────────────────────────────────────────────────────────────
// 内部胶水：把 emit 闭包编译成 JitFn<F>
// ──────────────────────────────────────────────────────────────

fn compile_stub<B, F, Body>(body: Body) -> JitFn<F>
where
    B: MacroAssemblerBackend,
    F: Copy,
    Body: FnOnce(&mut MacroAssembler<B>),
{
    let mut masm = MacroAssembler::<B>::new();
    body(&mut masm);
    unsafe { masm.compile() }
}

// ──────────────────────────────────────────────────────────────
// § 1  sum_array  —  一份 emit，所有平台共用
//
//  fn sum_array(ptr: *const T, len: isize) -> T
//
//  VReg:  Arg(0)=ptr  Arg(1)=len  Ret=acc  Cnt=i  Tmp(0)=elem
// ──────────────────────────────────────────────────────────────

fn emit_sum_array<B: MacroAssemblerBackend>(masm: &mut MacroAssembler<B>) {
    let ptr  = VReg::Arg(0);
    let len  = VReg::Arg(1);
    let acc  = VReg::Ret;
    let i    = VReg::Cnt;
    let elem = VReg::Tmp(0);

    masm.prologue();
    masm.zero(acc);
    masm.zero(i);

    let loop_start = masm.new_label();
    let done       = masm.new_label();

    masm.bind(&loop_start);
    masm.cmp(i, len);
    masm.jump_if(Cond::Ge, &done);

    masm.load_ptr_scaled(elem, ptr, i);
    masm.add(acc, acc, elem);
    masm.inc(i);
    masm.jump(&loop_start);

    masm.bind(&done);
    masm.epilogue();
    masm.ret();
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
pub fn build_sum_array() -> JitFn<unsafe extern "C" fn(*const i64, i64) -> i64> {
    compile_stub::<NativeBackend, _, _>(emit_sum_array)
}

#[cfg(target_arch = "arm")]
pub fn build_sum_array() -> JitFn<unsafe extern "C" fn(*const i32, i32) -> i32> {
    compile_stub::<NativeBackend, _, _>(emit_sum_array)
}

// ──────────────────────────────────────────────────────────────
// § 2  factorial  —  迭代阶乘
//
//  fn factorial(n: i64) -> i64
//
//  VReg:  Arg(0)=n  Ret=acc  Tmp(0)=常量1
// ──────────────────────────────────────────────────────────────

fn emit_factorial<B: MacroAssemblerBackend>(masm: &mut MacroAssembler<B>) {
    let n   = VReg::Arg(0);
    let acc = VReg::Ret;
    let one = VReg::Tmp(0);

    masm.prologue();
    masm.mov_imm(acc, 1);
    masm.mov_imm(one, 1);

    let loop_start = masm.new_label();
    let done       = masm.new_label();

    masm.bind(&loop_start);
    masm.cmp(n, one);
    masm.jump_if(Cond::Le, &done);

    masm.mul(acc, acc, n);
    masm.dec(n);
    masm.jump(&loop_start);

    masm.bind(&done);
    masm.epilogue();
    masm.ret();
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
pub fn build_factorial() -> JitFn<unsafe extern "C" fn(i64) -> i64> {
    compile_stub::<NativeBackend, _, _>(emit_factorial)
}

#[cfg(target_arch = "arm")]
pub fn build_factorial() -> JitFn<unsafe extern "C" fn(i32) -> i32> {
    compile_stub::<NativeBackend, _, _>(emit_factorial)
}

// ──────────────────────────────────────────────────────────────
// § 3  const_add  —  return x + 10（循环演示）
//
//  fn const_add(x: i64) -> i64
//
//  VReg:  Arg(0)=x  Cnt=i  Tmp(0)=零常量
// ──────────────────────────────────────────────────────────────

fn emit_const_add<B: MacroAssemblerBackend>(masm: &mut MacroAssembler<B>) {
    let x    = VReg::Arg(0);
    let i    = VReg::Cnt;
    let zero = VReg::Tmp(0);

    masm.prologue();
    masm.mov_imm(i, 10);
    masm.zero(zero);

    let loop_start = masm.new_label();
    let done       = masm.new_label();

    masm.bind(&loop_start);
    masm.cmp(i, zero);
    masm.jump_if(Cond::Eq, &done);

    masm.inc(x);
    masm.dec(i);
    masm.jump(&loop_start);

    masm.bind(&done);
    masm.mov(VReg::Ret, x);
    masm.epilogue();
    masm.ret();

}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
pub fn build_const_add() -> JitFn<unsafe extern "C" fn(i64) -> i64> {
    compile_stub::<NativeBackend, _, _>(emit_const_add)
}

#[cfg(target_arch = "arm")]
pub fn build_const_add() -> JitFn<unsafe extern "C" fn(i32) -> i32> {
    compile_stub::<NativeBackend, _, _>(emit_const_add)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sum_array() {
        println!("[*] Testing JIT sum_array ...");
        let jit_sum = build_sum_array();
        
        let data: Vec<i64> = vec![10, 20, 30, 40, 50];
        let expected: i64 = data.iter().sum();
        
        unsafe {
            let result = (jit_sum.get())(data.as_ptr(), data.len() as i64);
            assert_eq!(result, expected, "sum_array result mismatch!");
        }
        println!("  [✓] sum([10..50]) = {}", expected);
    }

    #[test]
    fn test_factorial() {
        println!("[*] Testing JIT factorial ...");
        let jit_fact = build_factorial();
        
        let test_cases = [
            (0, 1), (1, 1), (5, 120), (10, 3628800)
        ];

        for (input, expected) in test_cases {
            unsafe {
                let result = (jit_fact.get())(input);
                assert_eq!(result, expected, "factorial({}) failed!", input);
            }
        }
        println!("  [✓] 0!, 1!, 5!, 10! are all correct.");
    }

    #[test]
    fn test_const_add() {
        println!("[*] Testing JIT const_add (loop based) ...");
        let jit_add = build_const_add();
        
        let input = 100;
        let expected = 110; // 100 + 10 iterations of inc
        
        unsafe {
            let result = (jit_add.get())(input);
            assert_eq!(result, expected, "const_add failed!");
        }
        println!("  [✓] 100 + 10 = {}", expected);
    }

    #[test]
    fn test_comprehensive_hexdump() {
        // 打印字节码以验证跨平台生成的差异性
        println!("\n[!] Architecture: {:?}", std::env::consts::ARCH);
        
        // 这种方式需要你之前修复的 into_bytes 或类似的 view_code 逻辑
        println!("sum_array code size: {} bytes", build_sum_array().code_size());
    }
}

#[test]
fn visualize_all_stubs() {
    // 1. sum_array
    let jit_sum = build_sum_array();
    hex_disassemble("sum_array", jit_sum.as_bytes());

    // 2. factorial
    let jit_fact = build_factorial();
    hex_disassemble("factorial", jit_fact.as_bytes());

    // 3. const_add
    let jit_add = build_const_add();
    hex_disassemble("const_add", jit_add.as_bytes());
}