use capstone::prelude::*;
use cranelift::prelude::*;
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};
use std::mem;


// 模拟安全检查：数值不能超过 100
pub extern "C" fn rust_security_check(val: i64) -> i32 {
    if val > 100 {
        println!("!!! [安全警报] 发现非法大数: {}，拒绝执行！", val);
        0 // 失败
    } else {
        1 // 成功
    }
}

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
    pub fn new_with_builder(builder: JITBuilder) -> Self {
        let module = JITModule::new(builder);
        Self {
            builder_context: FunctionBuilderContext::new(),
            ctx: module.make_context(),
            module,
        }
    }
    pub fn compile_secure_sum(&mut self) -> (FuncId, JitFunction) {
        let i64_type = types::I64;
        let i32_type = types::I32;

        // 定义签名: fn(a, b) -> i64
        self.ctx.func.signature.params.push(AbiParam::new(i64_type));
        self.ctx.func.signature.params.push(AbiParam::new(i64_type));
        self.ctx.func.signature.returns.push(AbiParam::new(i64_type));

        // 声明外部检查函数: fn(i64) -> i32
        let mut check_sig = self.module.make_signature();
        check_sig.params.push(AbiParam::new(i64_type));
        check_sig.returns.push(AbiParam::new(i32_type));
        let check_func_id = self.module.declare_function("rust_security_check", Linkage::Import, &check_sig).unwrap();

        let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut self.builder_context);
        
        // 创建三个块：入口、通过检查、错误处理
        let entry_block = builder.create_block();
        let pass_block = builder.create_block();
        let panic_block = builder.create_block();

        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);

        let arg_a = builder.block_params(entry_block)[0];
        
        // --- 开始检查 ---
        let local_check_func = self.module.declare_func_in_func(check_func_id, &mut builder.func);
        let call_inst = builder.ins().call(local_check_func, &[arg_a]);
        let check_result = builder.inst_results(call_inst)[0];

        // 如果 check_result == 0 (失败)，跳转到 panic_block
        builder.ins().brif(check_result, pass_block, &[], panic_block, &[]);

        // --- 逻辑：Panic 块 ---
        builder.switch_to_block(panic_block);
        builder.seal_block(panic_block);
        // 在 JIT 中模拟自杀：触发一个陷阱（Trap）
        let my_trap = TrapCode::user(1).expect("Invalid trap code"); 
        builder.ins().trap(my_trap);

        // --- 逻辑：通过检查块 ---
        builder.switch_to_block(pass_block);
        builder.seal_block(pass_block);
        let arg_b = builder.block_params(entry_block)[1];
        let res = builder.ins().iadd(arg_a, arg_b);
        builder.ins().return_(&[res]);

        builder.finalize();

        // 编译... (此处省略重复的 define_function 和 finalize 逻辑)
        let id = self.module.declare_function("secure_sum", Linkage::Export, &self.ctx.func.signature).unwrap();
        self.module.define_function(id, &mut self.ctx).unwrap();
        let size = self.ctx.compiled_code().unwrap().code_buffer().len();
        self.module.finalize_definitions().unwrap();
        (id, JitFunction { ptr: self.module.get_finalized_function(id), size })
    }
}

#[test]
fn test_jit_security_panic() {
    let mut builder = JITBuilder::new(cranelift_module::default_libcall_names()).unwrap();
    builder.symbol("rust_security_check", rust_security_check as *const u8);
    let mut jit = SimpleJit::new_with_builder(builder); // 假设你封装了构造函数

    let (_, func_info) = jit.compile_secure_sum();
    type SumFn = unsafe extern "C" fn(i64, i64) -> i64;
    let func: SumFn = unsafe { mem::transmute(func_info.ptr) };

    // 情况 A：合法输入
    println!("调用测试 A (10, 20)...");
    assert_eq!(unsafe { func(10, 20) }, 30);

    // 情况 B：非法输入，预期触发 Trap (崩溃)
    println!("调用测试 B (999, 20)... 准备迎接 Panic");
    // 注意：trap 会导致当前进程接收到信号而退出，
    // 在标准单元测试中，你可以使用 #[should_panic] 来捕捉这种底层的硬件异常
    unsafe { func(999, 20) }; 
}