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
    pub fn compile_prototype_access(&mut self) -> (FuncId, JitFunction) {
        let ptr_type = self.module.target_config().pointer_type();
        let i64_type = types::I64;

        // 签名: fn(obj_ptr: *const JsObject) -> i64
        self.ctx.func.signature.params.push(AbiParam::new(ptr_type));
        self.ctx.func.signature.returns.push(AbiParam::new(i64_type));

        let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut self.builder_context);
        let entry_block = builder.create_block();
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);

        // 1. 获取 JsObject 的基地址
        let obj_ptr = builder.block_params(entry_block)[0];

        // 2. 第一级加载：获取 proto 指针 (在 JsObject 偏移 8 处)
        // 注意：这里 load 的类型是 ptr_type (64位系统上是 I64)
        let proto_ptr = builder.ins().load(ptr_type, MemFlags::new(), obj_ptr, 8);

        // 3. 第二级加载：获取 Prototype 里的 value (在 Prototype 偏移 8 处)
        let value = builder.ins().load(i64_type, MemFlags::new(), proto_ptr, 8);

        // 4. 返回结果
        builder.ins().return_(&[value]);

        builder.finalize();
        
        // ... 标准编译流程 ...
        let id = self.module.declare_function("get_proto_val", Linkage::Export, &self.ctx.func.signature).unwrap();
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

#[repr(C)]
pub struct Prototype {
    pub version: i64,   // 偏移 0
    pub value: i64,     // 偏移 8
}

#[repr(C)]
pub struct JsObject {
    pub id: i64,                // 偏移 0
    pub proto: *const Prototype, // 偏移 8 (指针)
}

#[test]
fn test_jit_prototype_chain() {
    let mut jit = SimpleJit::new();
    let (_, func_info) = jit.compile_prototype_access();

    jit.print_disassembly(&func_info);

    type ProtoFn = unsafe extern "C" fn(*const JsObject) -> i64;
    let func: ProtoFn = unsafe { std::mem::transmute(func_info.ptr) };

    // 1. 准备数据：先创建原型
    let my_proto = Prototype { version: 1, value: 999 };
    
    // 2. 创建主对象，指向原型
    let my_obj = JsObject {
        id: 42,
        proto: &my_proto as *const Prototype,
    };

    // 3. 调用 JIT
    let val = unsafe { func(&my_obj as *const JsObject) };

    println!("从原型链读取到的值: {}", val);
    assert_eq!(val, 999);
}