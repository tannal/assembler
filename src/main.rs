use capstone::prelude::*;
use cranelift::prelude::*;
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};
use std::mem;

pub struct SimpleJit {
    builder_context: FunctionBuilderContext,
    ctx: codegen::Context,
    module: JITModule,
}
pub struct JitFunction {
    pub ptr: *const u8,
    pub size: usize,
}

impl SimpleJit {
    pub fn new() -> Self {
        let builder = JITBuilder::new(cranelift_module::default_libcall_names()).unwrap();
        let module = JITModule::new(builder);
        Self {
            builder_context: FunctionBuilderContext::new(),
            ctx: module.make_context(),
            module,
        }
    }

    pub fn compile_sum_n(&mut self, n: usize) -> (FuncId, JitFunction) {
        let i64_type = types::I64;

        // 1. 动态定义 N 个参数的签名
        for _ in 0..n {
            self.ctx.func.signature.params.push(AbiParam::new(i64_type));
        }
        // 定义返回值
        self.ctx
            .func
            .signature
            .returns
            .push(AbiParam::new(i64_type));

        let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut self.builder_context);
        let entry_block = builder.create_block();

        // 2. 告诉 builder 在这个 block 中接收参数
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);

        // 3. 获取所有参数的 Value 句柄，并立即转换为 Vec 以释放 builder 的借用
        let params = builder.block_params(entry_block).to_vec();

        // 4. 循环求和逻辑
        let mut sum = builder.ins().iconst(i64_type, 0);
        for i in 0..n {
            let next_val = params[i]; // 此时 params 是独立的 Vec<Value>，不再借用 builder
            sum = builder.ins().iadd(sum, next_val); // builder 现在可以自由进行可变操作了
        }

        // 5. 返回总和
        builder.ins().return_(&[sum]);
        builder.finalize();

        // 编译流程
        let func_name = format!("sum_{}", n);
        let id = self
            .module
            .declare_function(&func_name, Linkage::Export, &self.ctx.func.signature)
            .unwrap();
        self.module.define_function(id, &mut self.ctx).unwrap();

        let compiled_info = self.ctx.compiled_code().expect("Code not compiled");
        let size = compiled_info.code_buffer().len();

        self.module.finalize_definitions().unwrap();
        let code_ptr = self.module.get_finalized_function(id);

        (
            id,
            JitFunction {
                ptr: code_ptr,
                size,
            },
        )
    }

    /// 编译并返回 FuncId 和机器码地址
    pub fn compile_add_mul(&mut self) -> (FuncId, JitFunction) {
        let i64_type = types::I64;

        self.ctx.func.signature.params.push(AbiParam::new(i64_type));
        self.ctx.func.signature.params.push(AbiParam::new(i64_type));
        self.ctx
            .func
            .signature
            .returns
            .push(AbiParam::new(i64_type));

        let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut self.builder_context);
        let entry_block = builder.create_block();

        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);

        let arg_a = builder.block_params(entry_block)[0];
        let arg_b = builder.block_params(entry_block)[1];

        let v0 = builder.ins().iadd(arg_a, arg_b);
        let imm_2 = builder.ins().iconst(i64_type, 2);
        let v1 = builder.ins().imul(v0, imm_2);

        builder.ins().return_(&[v1]);
        builder.finalize();

        // 打印 Cranelift IR (中间表示)，这在调试寄存器分配时非常有用
        println!("--- Cranelift IR ---\n{}", self.ctx.func.display());

        let id = self
            .module
            .declare_function("test_fn", Linkage::Export, &self.ctx.func.signature)
            .unwrap();
        self.module.define_function(id, &mut self.ctx).unwrap();

        // 获取编译产物的详细信息
        let compiled_info = self.ctx.compiled_code().expect("Code not compiled");
        let size = compiled_info.code_buffer().len();

        self.module.finalize_definitions().unwrap();
        let code_ptr = self.module.get_finalized_function(id);

        (
            id,
            JitFunction {
                ptr: code_ptr,
                size,
            },
        )
    }

    pub fn compile_sum_and_print(&mut self, n: usize) -> (FuncId, JitFunction) {
        let i64_type = types::I64;

        // 1. 定义主函数签名: fn(p1, ..., pN) -> void (这里我们让它不返回，直接打印)
        for _ in 0..n {
            self.ctx.func.signature.params.push(AbiParam::new(i64_type));
        }

        // 2. 声明外部函数签名: fn(i64) -> void
        let mut print_sig = self.module.make_signature();
        print_sig.params.push(AbiParam::new(i64_type));

        // 3. 在模块中声明外部函数 (Import)
        let print_func_id = self.module
            .declare_function("rust_print_sum", Linkage::Import, &print_sig)
            .expect("声明外部函数失败");

        let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut self.builder_context);
        let entry_block = builder.create_block();
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);

        // 4. 求和逻辑
        let params = builder.block_params(entry_block).to_vec();
        let mut sum = builder.ins().iconst(i64_type, 0);
        for val in params {
            sum = builder.ins().iadd(sum, val);
        }

        // 5. 调用外部函数
        // 首先需要将全局的外部函数 ID 映射到当前函数的本地引用
        let local_print_func = self.module.declare_func_in_func(print_func_id, &mut builder.func);
        builder.ins().call(local_print_func, &[sum]);

        builder.ins().return_(&[]);
        builder.finalize();

        // 6. 编译
        let id = self.module.declare_function("main_jit", Linkage::Export, &self.ctx.func.signature).unwrap();
        self.module.define_function(id, &mut self.ctx).unwrap();

        let size = self.ctx.compiled_code().unwrap().code_buffer().len();
        self.module.finalize_definitions().unwrap();
        
        let code_ptr = self.module.get_finalized_function(id);
        (id, JitFunction { ptr: code_ptr, size })
    }

    /// 执行反汇编打印
    pub fn print_disassembly(&self, func: &JitFunction) {
        let cs = Capstone::new()
            .x86()
            .mode(arch::x86::ArchMode::Mode64)
            .syntax(arch::x86::ArchSyntax::Intel)
            .build()
            .unwrap();

        unsafe {
            // 使用精确的 size 创建 slice
            let slice = std::slice::from_raw_parts(func.ptr, func.size);
            let insns = cs.disasm_all(slice, func.ptr as u64).unwrap();

            println!("--- 精确反汇编 (大小: {} 字节) ---", func.size);
            for i in insns.iter() {
                println!(
                    "  0x{:x}: {:10} {}",
                    i.address(),
                    i.mnemonic().unwrap(),
                    i.op_str().unwrap()
                );
            }
        }
    }
}

// 这是一个普通的 Rust 函数，我们将让 JIT 代码调用它
pub extern "C" fn rust_print_sum(result: i64) {
    println!(">>> [来自 Rust 的回调] 计算结果是: {}", result);
}

#[test]
fn test_jit_external_call() {
    let mut builder = JITBuilder::new(cranelift_module::default_libcall_names()).unwrap();
    
    // 【关键】将 Rust 函数名映射到其实际内存地址
    builder.symbol("rust_print_sum", rust_print_sum as *const u8);

    let module = JITModule::new(builder);
    let mut jit = SimpleJit {
        builder_context: FunctionBuilderContext::new(),
        ctx: module.make_context(),
        module,
    };

    let num_params = 10;
    let (id, func_info) = jit.compile_sum_and_print(num_params);

    jit.print_disassembly(&func_info);

    // 定义函数签名 (无返回值)
    type JitMainFn = unsafe extern "C" fn(i64, i64, i64, i64, i64, i64, i64, i64, i64, i64);
    let func: JitMainFn = unsafe { mem::transmute(func_info.ptr) };

    println!("--- 开始执行 JIT 代码 ---");
    unsafe { func(1, 1, 1, 1, 1, 1, 1, 1, 1, 1) }; // 结果应该是 10
    println!("--- JIT 执行结束 ---");
}

#[cfg(test)]
mod tests {
    use super::*; // 确保 SimpleJit 和相关结构体在作用域内
    use std::mem;

    #[test]
    fn test_jit_sum_10_arguments() {
        let mut jit = SimpleJit::new();
        let num_params = 10;
        
        // 1. 编译函数
        let (id, func_info) = jit.compile_sum_n(num_params);

        // 2. 打印反汇编（可选，仅用于手动验证，测试运行时通常加 --nocapture）
        jit.print_disassembly(&func_info);

        // 3. 定义函数签名
        // 使用 extern "C" 确保遵循 C 调用约定
        type SumFn = unsafe extern "C" fn(i64, i64, i64, i64, i64, i64, i64, i64, i64, i64) -> i64;
        
        // 4. 类型转换
        let func: SumFn = unsafe { mem::transmute(func_info.ptr) };

        // 5. 执行并断言
        let res = unsafe { func(1, 2, 3, 4, 5, 6, 7, 8, 9, 10) };
        
        println!("JIT 返回结果: {}", res);
        assert_eq!(res, 55, "1 到 10 的累加和应该是 55");
    }

    #[test]
    fn test_jit_sum_small_arguments() {
        let mut jit = SimpleJit::new();
        let num_params = 3;
        let (id, func_info) = jit.compile_sum_n(num_params);

        type Sum3Fn = unsafe extern "C" fn(i64, i64, i64) -> i64;
        let func: Sum3Fn = unsafe { mem::transmute(func_info.ptr) };

        let res = unsafe { func(10, 20, 30) };
        assert_eq!(res, 60);
    }
}

fn main() {
    let mut jit = SimpleJit::new();
    let (id, func_info) = jit.compile_add_mul();

    // 打印反汇编
    jit.print_disassembly(&func_info);

    // 调用函数
    let func: fn(i64, i64) -> i64 = unsafe { mem::transmute(func_info.ptr) };
    let res = func(10, 5);
    println!("\n调用结果: (10 + 5) * 2 = {}", res);
}
