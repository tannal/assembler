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
    pub fn compile_dynamic_dispatch(&mut self) -> (FuncId, JitFunction) {
        let i64_type = types::I64;

        // 签名: fn(val_ptr: *const JsValue) -> i64
        self.ctx.func.signature.params.push(AbiParam::new(i64_type));
        self.ctx
            .func
            .signature
            .returns
            .push(AbiParam::new(i64_type));

        let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut self.builder_context);

        // 定义基本块
        let entry_block = builder.create_block();
        let int_block = builder.create_block();
        let str_block = builder.create_block();

        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);

        let val_ptr = builder.block_params(entry_block)[0];

        // 1. 加载 Tag (偏移 0)
        let tag = builder.ins().load(i64_type, MemFlags::new(), val_ptr, 0);

        // 2. 比较 Tag：如果是 1 (Int) 走 int_block，否则走 str_block
        let is_int = builder.ins().icmp_imm(IntCC::Equal, tag, 1);
        builder.ins().brif(is_int, int_block, &[], str_block, &[]);

        // --- Int 分支 ---
        builder.switch_to_block(int_block);
        builder.seal_block(int_block);
        let int_val = builder.ins().load(i64_type, MemFlags::new(), val_ptr, 8);
        builder.ins().return_(&[int_val]);

        // --- String 分支 ---
        builder.switch_to_block(str_block);
        builder.seal_block(str_block);
        // 这里可以实现调用外部 Rust 函数打印字符串的逻辑
        let zero = builder.ins().iconst(i64_type, 0);
        builder.ins().return_(&[zero]);

        builder.finalize();

        // ... 标准编译流程 ...
        let id = self
            .module
            .declare_function("dispatch", Linkage::Export, &self.ctx.func.signature)
            .unwrap();
        self.module.define_function(id, &mut self.ctx).unwrap();
        self.module.finalize_definitions().unwrap();
        (
            id,
            JitFunction {
                ptr: self.module.get_finalized_function(id),
                size: 0,
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

#[repr(C)]
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
fn test_jit_dynamic_tag() {
    let mut jit = SimpleJit::new();
    let (_, func_info) = jit.compile_dynamic_dispatch();

    type DispatchFn = unsafe extern "C" fn(*const JsValue) -> i64;
    let func: DispatchFn = unsafe { std::mem::transmute(func_info.ptr) };
    
    // 测试 A: 传入整数
    let val_int = JsValue { tag: JsTag::Int, data: 888 };
    assert_eq!(unsafe { func(&val_int) }, 888);
    
    // 测试 B: 传入字符串 (此时 JIT 返回 0)
    let val_str = JsValue { tag: JsTag::String, data: 0 };
    assert_eq!(unsafe { func(&val_str) }, 0);
    
    jit.print_disassembly(&func_info);
    println!("动态类型分发测试通过！");
}