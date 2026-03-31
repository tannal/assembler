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

    pub fn compile_with_inline_cache(&mut self) -> (FuncId, JitFunction) {
        let i64_type = types::I64;

        // --- 关键修正：先填入函数的参数和返回签名 ---
        self.ctx.func.signature.params.clear(); // 清理旧数据
        self.ctx.func.signature.returns.clear();
        self.ctx.func.signature.params.push(AbiParam::new(i64_type)); // obj_ptr
        self.ctx
            .func
            .signature
            .returns
            .push(AbiParam::new(i64_type)); // 返回值

        // 1. 声明外部缓存地址和更新函数
        // 我们通过符号名 "LAST_SEEN_TAG" 来获取它的地址
        let cache_data_id = self
            .module
            .declare_data("LAST_SEEN_TAG", Linkage::Import, false, false)
            .unwrap();

        let mut update_sig = self.module.make_signature();
        update_sig.params.push(AbiParam::new(i64_type));
        let update_func_id = self
            .module
            .declare_function("update_type_cache", Linkage::Import, &update_sig)
            .unwrap();

        let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut self.builder_context);
        let entry_block = builder.create_block();
        let fast_path = builder.create_block();
        let slow_path = builder.create_block();

        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);

        let obj_ptr = builder.block_params(entry_block)[0];
        let actual_tag = builder.ins().load(i64_type, MemFlags::new(), obj_ptr, 0);

        // --- 核心：从全局地址读取缓存的 Tag ---
        let global_val = self
            .module
            .declare_data_in_func(cache_data_id, &mut builder.func);
        let cache_addr = builder.ins().symbol_value(i64_type, global_val);
        let cached_tag = builder.ins().load(i64_type, MemFlags::new(), cache_addr, 0);

        // 比较：实际 Tag 是否等于缓存的 Tag？
        let is_match = builder.ins().icmp(IntCC::Equal, actual_tag, cached_tag);
        builder.ins().brif(is_match, fast_path, &[], slow_path, &[]);

        // --- Fast Path: 直接计算 ---
        builder.switch_to_block(fast_path);
        builder.seal_block(fast_path);
        let data = builder.ins().load(i64_type, MemFlags::new(), obj_ptr, 8);
        let res = builder.ins().iadd_imm(data, 100);
        builder.ins().return_(&[res]);

        // --- Slow Path: 更新缓存并处理 ---
        builder.switch_to_block(slow_path);
        builder.seal_block(slow_path);
        let local_update_fn = self
            .module
            .declare_func_in_func(update_func_id, &mut builder.func);
        builder.ins().call(local_update_fn, &[actual_tag]);
        // 这里模拟回滚，返回 -1 表示需要重新编译或解释执行
        let error_val = builder.ins().iconst(i64_type, -1);
        builder.ins().return_(&[error_val]);

        builder.finalize();
        // ... 标准编译流程 ...
        let id = self
            .module
            .declare_function("inline_cache", Linkage::Export, &self.ctx.func.signature)
            .unwrap();
        self.module.define_function(id, &mut self.ctx).unwrap();
        let size = self.ctx.compiled_code().unwrap().code_buffer().len();
        self.module.finalize_definitions().unwrap();
        (
            id,
            JitFunction {
                ptr: self.module.get_finalized_function(id),
                size,
            },
        )
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
// 模拟 JIT 的类型反馈槽 (Type Feedback Slot)
pub static mut LAST_SEEN_TAG: i64 = 1; // 默认猜测是 Int (1)

// 当猜测失败时，调用此函数“学习”新类型
pub extern "C" fn update_type_cache(actual_tag: i64) {
    unsafe {
        println!(
            "核心反馈：类型已从 {} 变为 {}，更新缓存...",
            LAST_SEEN_TAG, actual_tag
        );
        LAST_SEEN_TAG = actual_tag;
    }
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

#[test]
fn test_jit_learning_cache() {
    let mut builder = JITBuilder::new(cranelift_module::default_libcall_names()).unwrap();
    // 关键：将 Rust 的全局变量地址暴露给 JIT
    unsafe {
        builder.symbol("LAST_SEEN_TAG", &LAST_SEEN_TAG as *const i64 as *const u8);
    }
    builder.symbol("update_type_cache", update_type_cache as *const u8);

    let mut jit = SimpleJit::new_with_builder(builder);
    let (_, func_info) = jit.compile_with_inline_cache();
    type CacheFn = unsafe extern "C" fn(*const JsValue) -> i64;
    let func: CacheFn = unsafe { std::mem::transmute(func_info.ptr) };

    jit.print_disassembly(&func_info);

    // 1. 初始缓存是 1 (Int)，传入 Int -> 命中快路径
    let val_int = JsValue {
        tag: JsTag::Int,
        data: 500,
    };
    assert_eq!(unsafe { func(&val_int) }, 600);
    println!("第一次调用：快路径命中！");

    // 2. 传入 String (Tag 2) -> 缓存失效，触发学习
    let val_str = JsValue {
        tag: JsTag::String,
        data: 0,
    };
    let res = unsafe { func(&val_str) };
    assert_eq!(res, -1); // 触发了慢路径返回

    // 3. 再次传入 String -> 此时缓存应该已经是 2 了！
    // 注意：在真实 JIT 中，此时我们会重新生成一段针对 String 的代码
    // 这里我们验证缓存确实变了
    unsafe {
        assert_eq!(LAST_SEEN_TAG, 2);
    }
    println!("第二次调用：缓存已自动更新为 String (2)！");
}
