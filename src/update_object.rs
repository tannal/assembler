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
    pub fn compile_read_balance(&mut self) -> (FuncId, JitFunction) {
        let i64_type = types::I64;

        // 定义签名: fn(user_ptr: *const UserData) -> i64
        self.ctx.func.signature.params.push(AbiParam::new(i64_type)); // 传入指针
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

        // 1. 获取基地址 (UserData 指针)
        let user_ptr = builder.block_params(entry_block)[0];

        // 2. 加载旧余额 (偏移量 8，跳过 8 字节的 id)
        let old_balance = builder.ins().load(i64_type, MemFlags::new(), user_ptr, 8);

        // 3. 计算新余额：old_balance + 100
        let new_balance = builder.ins().iadd_imm(old_balance, 100);

        // 4. 写回内存 (Store)
        builder
            .ins()
            .store(MemFlags::new(), new_balance, user_ptr, 8);

        // 5. 返回旧余额
        builder.ins().return_(&[old_balance]);

        builder.finalize();

        // ... 编译与返回逻辑 (与之前相同) ...
        let id = self
            .module
            .declare_function("object_load", Linkage::Export, &self.ctx.func.signature)
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

#[repr(C)]
pub struct UserData {
    pub id: i64,      // 偏移 0
    pub balance: i64, // 偏移 8
}

#[test]
fn test_jit_memory_access() {
    let mut jit = SimpleJit::new();
    let (_, func_info) = jit.compile_read_balance();

    // 打印反汇编，你应该能看到一条带有 [rcx + 8] 的 mov 指令
    jit.print_disassembly(&func_info);

    // 转换为函数指针
    type UpdateFn = unsafe extern "C" fn(*mut UserData) -> i64;
    let func: UpdateFn = unsafe { std::mem::transmute(func_info.ptr) };

    // 1. 在 Rust 中创建一个结构体实例
    let mut my_user = UserData {
        id: 12345,
        balance: 500,
    };

    println!("--- 执行前 ---");
    println!("ID: {}, Balance: {}", my_user.id, my_user.balance);

    // 2. 将结构体指针传给 JIT
    let user_ptr = &mut my_user as *mut UserData;
    let old_val = unsafe { func(user_ptr) };

    println!("--- 执行后 ---");
    println!("JIT 返回的旧余额: {}", old_val);
    println!("Rust 侧看到的最新余额: {}", my_user.balance);

    // 3. 验证结果
    assert_eq!(old_val, 500);
    assert_eq!(my_user.balance, 600);
    assert_eq!(my_user.id, 12345, "ID 不应该被修改");
}
