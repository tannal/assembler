// ──────────────────────────────────────────────────────────────
// § Fibonacci  —  迭代实现
//
//  fn fib(n: i64) -> i64
//
//  VReg:  Arg(0)=n  Ret=curr  Tmp(0)=prev  Tmp(1)=next  Tmp(2)=常量1
// ──────────────────────────────────────────────────────────────

use crate::{macro_asm::{Cond, MacroAssembler, MacroAssemblerBackend, VReg}, util::hexdump::hex_disassemble};
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
use crate::runtime::JitFn;

fn emit_fibonacci<B: MacroAssemblerBackend>(masm: &mut MacroAssembler<B>) {
    let n     = VReg::Arg(0);
    let curr  = VReg::Ret;   // 结果存放于返回值寄存器 (F_i)
    let prev  = VReg::Tmp(0); // 前一个数 (F_{i-1})
    let next  = VReg::Tmp(1); // 临时交换变量
    let one   = VReg::Tmp(2); // 常量 1

    masm.prologue();
    
    // 基础情况处理: if n <= 1 return n
    let base_case = masm.new_label();
    masm.mov_imm(one, 1);
    masm.cmp(n, one);
    masm.jump_if(Cond::Le, &base_case);

    // 初始化: prev = 0, curr = 1
    masm.zero(prev);
    masm.mov_imm(curr, 1);
    
    // 我们从 i = 1 开始，循环到 n
    // 循环次数为 n - 1
    masm.dec(n);

    let loop_start = masm.new_label();
    let done       = masm.new_label();

    masm.bind(&loop_start);
    // 检查循环计数器 n 是否减为 0
    masm.zero(next); // 借用 next 临时存 0
    masm.cmp(n, next);
    masm.jump_if(Cond::Eq, &done);

    // 核心逻辑:
    // next = curr + prev
    // prev = curr
    // curr = next
    masm.add(next, curr, prev);
    masm.mov(prev, curr);
    masm.mov(curr, next);

    masm.dec(n);
    masm.jump(&loop_start);

    masm.bind(&base_case);
    masm.mov(VReg::Ret, n); // 如果 n <= 1，直接返回 n

    masm.bind(&done);
    masm.epilogue();
    masm.ret();
}

// ──────────────────────────────────────────────────────────────
// § Fibonacci  —  递归实现
//
//  fn fib_rec(n: i64) -> i64 {
//      if n <= 1 { return n; }
//      return fib_rec(n - 1) + fib_rec(n - 2);
//  }
//
//  VReg:  Arg(0)=n  Ret=res  Tmp(0)=f1  Tmp(1)=1(const)
// ──────────────────────────────────────────────────────────────

fn emit_fibonacci_recursive<B: MacroAssemblerBackend>(masm: &mut MacroAssembler<B>) {
    let n    = VReg::Arg(0);
    let res  = VReg::Ret;
    let f1   = VReg::Tmp(0);
    let one  = VReg::Tmp(1);

    let entry = masm.new_label();
    let done  = masm.new_label();

    masm.bind(&entry);

    // ── 1. 边界检查 (Base Case) ──────────────────────────
    // 注意：在 prologue 之前检查可以省去不必要的建栈开销
    masm.mov_imm(one, 1);
    masm.cmp(n, one);
    masm.jump_if(Cond::Le, &done); // n <= 1 则跳转到 done (此时 res 映射到 rax, n 在 arg0)

    // ── 2. 建栈 + 保护参数 ────────────────────────────────
    masm.prologue();
    masm.push_vreg(n); // 将原始 n 压栈保护

    // ── 3. 计算 fib(n - 1) ────────────────────────────────
    masm.dec(n);       // n = n - 1
    masm.call_label(&entry);

    // ── 4. 状态切换：准备计算 fib(n - 2) ──────────────────
    masm.pop_vreg(n);  // 恢复原始 n (例如 2)
    masm.push_vreg(res); // 保护 fib(n-1) 的结果到栈上
    masm.push_vreg(n); // 再次保护 n，因为第二次 call 还会破坏它 (为了平栈一致性)

    masm.sub_imm(n, 2); // 计算 n - 2
    masm.call_label(&entry);

    // ── 5. 结果汇总 ───────────────────────────────────────
    // 此时 res (RAX) 已经是 fib(n-2) 的结果
    masm.pop_vreg(n);  // 弹出之前 push 的 n (平栈)
    masm.pop_vreg(f1); // 弹出之前 push 的 fib(n-1) 到 f1
    masm.add(res, res, f1); // res = fib(n-2) + fib(n-1)

    // ── 6. 清理返回 ───────────────────────────────────────
    masm.epilogue();

    masm.bind(&done);
    // 如果是从 base case 直接跳过来的，确保返回值正确
    // 逻辑：if n <= 1 { res = n }
    let skip_base = masm.new_label();
    masm.mov_imm(one, 1);
    masm.cmp(n, one);
    masm.jump_if(Cond::Gt, &skip_base);
    masm.mov(res, n); 
    masm.bind(&skip_base);

    masm.ret();
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
pub fn build_fibonacci() -> JitFn<unsafe extern "C" fn(i64) -> i64> {
    use crate::{macro_asm::NativeBackend, stubs::compile_stub};

    compile_stub::<NativeBackend, _, _>(emit_fibonacci)
}

#[cfg(target_arch = "arm")]
pub fn build_fibonacci() -> JitFn<unsafe extern "C" fn(i32) -> i32> {
    compile_stub::<NativeBackend, _, _>(emit_fibonacci)
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
pub fn build_fibonacci_recursive() -> JitFn<unsafe extern "C" fn(i64) -> i64> {
    use crate::{macro_asm::NativeBackend, stubs::compile_stub};

    compile_stub::<NativeBackend, _, _>(emit_fibonacci_recursive)
}

#[cfg(target_arch = "arm")]
pub fn build_fibonacci_recursive() -> JitFn<unsafe extern "C" fn(i32) -> i32> {
    compile_stub::<NativeBackend, _, _>(emit_fibonacci_recursive)
}

#[test]
fn test_jit_fibonacci() {
    println!("\n[*] Testing JIT Fibonacci (Iterative) ...");
    let jit_fib = build_fibonacci();

    let test_cases = [
        (0, 0), (1, 1), (2, 1), (3, 2), (4, 3), 
        (5, 5), (6, 8), (10, 55), (20, 6765)
    ];

    for (input, expected) in test_cases {
        unsafe {
            let result = (jit_fib.get())(input);
            assert_eq!(result, expected, "fib({}) failed!", input);
        }
    }
    hex_disassemble("fibonacci Iterative", jit_fib.as_bytes());
    println!("  [✓] Fibonacci sequence up to F(20) is correct.");
}

#[test]
fn test_jit_fibonacci_recursive() {
    println!("\n[*] Testing JIT Fibonacci (Recursive) ...");
    let jit_fib = build_fibonacci_recursive();

    let test_cases = [
        (0, 0), (1, 1), (2, 1), (3, 2), (4, 3),
        (5, 5), (6, 8), (10, 55), (20, 6765)
    ];

    for (input, expected) in test_cases {
        unsafe {
            let result = (jit_fib.get())(input);
            assert_eq!(result, expected, "fib({}) failed!", input);
        }
    }
    hex_disassemble("fibonacci recursive", jit_fib.as_bytes());
    println!("  [✓] Fibonacci sequence up to F(20) is correct.");
}