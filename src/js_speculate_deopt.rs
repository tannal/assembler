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

    pub fn new_with_builder(builder: JITBuilder) -> Self {
        let module = JITModule::new(builder);

        Self {
            builder_context: FunctionBuilderContext::new(),
            ctx: module.make_context(),
            module,
        }
    }

    pub fn compile_speculative_int_add(&mut self) -> (FuncId, JitFunction) {
        let i64_type = types::I64;

        // 定义签名: fn(val_ptr: *const JsValue) -> i64
        self.ctx.func.signature.params.push(AbiParam::new(i64_type));
        self.ctx.func.signature.returns.push(AbiParam::new(i64_type));

        // 声明外部 Bailout 函数: fn(expected: i64, actual: i64)
        let mut deopt_sig = self.module.make_signature();
        deopt_sig.params.push(AbiParam::new(i64_type)); // expected
        deopt_sig.params.push(AbiParam::new(i64_type)); // actual
        let deopt_func_id = self.module.declare_function("jit_deopt_bailout", Linkage::Import, &deopt_sig).unwrap();

        let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut self.builder_context);
        let entry_block = builder.create_block();
        let add_block = builder.create_block(); // 高效路径
        let deopt_block = builder.create_block(); // 回滚路径

        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);

        let val_ptr = builder.block_params(entry_block)[0];

        // 1. 【类型守卫】加载 Tag (偏移 0)
        let tag = builder.ins().load(i64_type, MemFlags::new(), val_ptr, 0);

        // 2. 【单态猜测】比较 Tag：猜测它一定是 1 (JsTag::Int)
        // 这个 `cmp` 指令会被 CPU 的分支预测器处理，由于总是匹配，预测成功率极高
        let is_int = builder.ins().icmp_imm(IntCC::Equal, tag, 1);
        builder.ins().brif(is_int, add_block, &[], deopt_block, &[]);

        // --- 逻辑：高效的 Int 加法路径 ---
        builder.switch_to_block(add_block);
        builder.seal_block(add_block);
        // 直接加载 data 并加 100，没有任何其他检查
        let int_data = builder.ins().load(i64_type, MemFlags::new(), val_ptr, 8);
        let res = builder.ins().iadd_imm(int_data, 100);
        builder.ins().return_(&[res]);

        // --- 逻辑：Deopt 回滚路径 ---
        builder.switch_to_block(deopt_block);
        builder.seal_block(deopt_block);
        let expected_tag = builder.ins().iconst(i64_type, 1); // 预期是 Int (1)
        let local_deopt_func = self.module.declare_func_in_func(deopt_func_id, &mut builder.func);
        // 调用 Rust 完成回滚
        builder.ins().call(local_deopt_func, &[expected_tag, tag]); 
        // 这一步在真实引擎中不会执行，因为 call 内部已经跳转回解释器
        let my_trap = TrapCode::user(1).expect("Invalid trap code");
        builder.ins().trap(my_trap); 

        builder.finalize();
        
        // ... 标准编译流程 ...
        let id = self.module.declare_function("speculative_add", Linkage::Export, &self.ctx.func.signature).unwrap();
        self.module.define_function(id, &mut self.ctx).unwrap();
        let size = self.ctx.compiled_code().unwrap().code_buffer().len();
        self.module.finalize_definitions().unwrap();
        (id, JitFunction { ptr: self.module.get_finalized_function(id), size })
    }

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
// 模拟 V8 的 Deoptimization Bailout
pub extern "C" fn jit_deopt_bailout(expected_tag: i64, actual_tag: i64) {
    eprintln!(
        "!!! [JIT Deopt] 猜测失败！预期 Tag: {}, 实际 Tag: {}。正在执行回滚 (模拟 Panic)...",
        expected_tag, actual_tag
    );
    // 在真实引擎中，这里会恢复解释器状态并继续执行
    // 这里我们直接 panic 来模拟去优化过程
    panic!("JIT Speculation Failed");
}
#[repr(i64)]
pub enum JsTag {
    Int = 1,
    String = 2,
}

#[repr(C)]
pub struct JsValue {
    pub tag: JsTag, // 偏移 0
    pub data: i64,  // 偏移 8 (如果是 Int 则是值，如果是 String 则是指针)
}

// 假设你之前定义了 #[repr(C)] JsValue 和 JsTag

#[test]
fn test_jit_speculative_optimization() {
    let mut builder = cranelift_jit::JITBuilder::new(cranelift_module::default_libcall_names()).unwrap();
    // 注入 Deopt 函数地址
    builder.symbol("jit_deopt_bailout", jit_deopt_bailout as *const u8);
    let mut jit = SimpleJit::new_with_builder(builder); 

    let (_, func_info) = jit.compile_speculative_int_add();
    jit.print_disassembly(&func_info);

    type SpecFn = unsafe extern "C" fn(*const JsValue) -> i64;
    let func: SpecFn = unsafe { std::mem::transmute(func_info.ptr) };

    // 场景 A: 猜测成功 (传入 Int)
    println!("--- 调用场景 A (Int) ---");
    let val_int = JsValue { tag: JsTag::Int, data: 500 };
    let res = unsafe { func(&val_int) };
    assert_eq!(res, 600);
    println!("猜测成功，高效路径执行完毕。");

    // 场景 B: 猜测失败 (传入 String)，预期 Panic (Deopt)
    println!("--- 调用场景 B (String) --- 准备迎接 Deopt Panic");
    let val_str = JsValue { tag: JsTag::String, data: 0 };
    unsafe { func(&val_str as *const JsValue) };
    // 这一步会执行 JIT 代码里的 deopt 分支，调用 jit_deopt_bailout，最后在 Rust 侧 panic
    // 在测试框架中，你可以用 #[should_panic] 捕捉它
    // unsafe { func(&val_str) }; 
}