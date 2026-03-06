use std::collections::HashMap;
use std::rc::Rc;

use crate::ast;
use crate::bytecode::{CodeObject, FunctionObj, Op, Value};
use crate::symbol::{Interner, Symbol};
use crate::ast::BinOp;

struct LoopContext {
    continue_target: usize,
    break_patches: Vec<usize>,
}

struct CodeBuilder {
    instructions: Vec<Op>,
    constants: Vec<Value>,
    local_slots: HashMap<Symbol, u16>,
    num_locals: u16,
    is_function: bool,
    loop_stack: Vec<LoopContext>,
}

impl CodeBuilder {
    fn new(is_function: bool) -> Self {
        CodeBuilder {
            instructions: Vec::new(),
            constants: Vec::new(),
            local_slots: HashMap::new(),
            num_locals: 0,
            is_function,
            loop_stack: Vec::new(),
        }
    }

    fn emit(&mut self, op: Op) -> usize {
        let idx = self.instructions.len();
        self.instructions.push(op);
        idx
    }

    fn add_const(&mut self, val: Value) -> u16 {
        // Reuse existing constants for integers and small values
        for (i, c) in self.constants.iter().enumerate() {
            match (&val, c) {
                (Value::Integer(a), Value::Integer(b)) if a == b => return i as u16,
                (Value::Boolean(a), Value::Boolean(b)) if a == b => return i as u16,
                (Value::None, Value::None) => return i as u16,
                _ => {}
            }
        }
        let idx = self.constants.len() as u16;
        self.constants.push(val);
        idx
    }

    fn current_offset(&self) -> usize {
        self.instructions.len()
    }

    fn patch_jump(&mut self, instr_idx: usize, target: usize) {
        let target = target as u32;
        match &mut self.instructions[instr_idx] {
            Op::Jump(ref mut t) => *t = target,
            Op::PopJumpIfFalse(ref mut t) => *t = target,
            Op::JumpIfTrueOrPop(ref mut t) => *t = target,
            Op::JumpIfFalseOrPop(ref mut t) => *t = target,
            _ => panic!("patch_jump on non-jump instruction"),
        }
    }

    fn alloc_hidden_local(&mut self) -> u16 {
        let slot = self.num_locals;
        self.num_locals += 1;
        slot
    }

    fn resolve_name(&self, name: Symbol) -> bool {
        // Returns true if name is a local slot
        self.is_function && self.local_slots.contains_key(&name)
    }

    fn emit_load(&mut self, name: Symbol) {
        if let Some(&slot) = self.local_slots.get(&name) {
            self.emit(Op::LoadLocal(slot));
        } else {
            self.emit(Op::LoadGlobal(name));
        }
    }

    fn emit_store(&mut self, name: Symbol) {
        if let Some(&slot) = self.local_slots.get(&name) {
            self.emit(Op::StoreLocal(slot));
        } else {
            self.emit(Op::StoreGlobal(name));
        }
    }

    fn build(mut self) -> CodeObject {
        self.peephole_optimize();
        CodeObject {
            instructions: self.instructions,
            constants: self.constants,
            num_locals: self.num_locals,
            local_slots: self.local_slots,
        }
    }

    fn peephole_optimize(&mut self) {
        // Collect all jump targets so we don't fuse across them
        let mut jump_targets = vec![false; self.instructions.len() + 1];
        for op in &self.instructions {
            match op {
                Op::Jump(t) | Op::PopJumpIfFalse(t)
                | Op::JumpIfTrueOrPop(t) | Op::JumpIfFalseOrPop(t) => {
                    let t = *t as usize;
                    if t < jump_targets.len() {
                        jump_targets[t] = true;
                    }
                }
                _ => {}
            }
        }

        let mut i = 0;
        while i + 3 < self.instructions.len() {
            // Don't fuse if instruction i+1, i+2, or i+3 is a jump target
            let any_target = (i + 1 < jump_targets.len() && jump_targets[i + 1])
                || (i + 2 < jump_targets.len() && jump_targets[i + 2])
                || (i + 3 < jump_targets.len() && jump_targets[i + 3]);
            if any_target {
                i += 1;
                continue;
            }

            // Pattern: LoadLocal(s) + LoadConst(c) + BinaryAdd + StoreLocal(s)
            // where const is integer → IncrLocalByConst
            if let (
                Op::LoadLocal(s1),
                Op::LoadConst(c),
                Op::BinaryAdd,
                Op::StoreLocal(s2),
            ) = (
                self.instructions[i],
                self.instructions[i + 1],
                self.instructions[i + 2],
                self.instructions[i + 3],
            ) {
                if s1 == s2 {
                    if matches!(self.constants.get(c as usize), Some(Value::Integer(_))) {
                        // Remap jump targets pointing into the fused region
                        self.instructions[i] = Op::IncrLocalByConst(Op::pack_pair(s1, c));
                        self.remove_instructions(i + 1, 3, &mut jump_targets);
                        continue;
                    }
                }
            }

            // Pattern: LoadLocal(a) + LoadLocal(b) + CompareLt + PopJumpIfFalse(t)
            if let (
                Op::LoadLocal(a),
                Op::LoadLocal(b),
                Op::CompareLt,
                Op::PopJumpIfFalse(t),
            ) = (
                self.instructions[i],
                self.instructions[i + 1],
                self.instructions[i + 2],
                self.instructions[i + 3],
            ) {
                self.instructions[i] = Op::LocalLtLocalJump(Op::pack_pair(a, b), t);
                self.remove_instructions(i + 1, 3, &mut jump_targets);
                continue;
            }

            // Pattern: LoadLocal(s) + LoadConst(c) + CompareLtE + PopJumpIfFalse(t)
            if let (
                Op::LoadLocal(s),
                Op::LoadConst(c),
                Op::CompareLtE,
                Op::PopJumpIfFalse(t),
            ) = (
                self.instructions[i],
                self.instructions[i + 1],
                self.instructions[i + 2],
                self.instructions[i + 3],
            ) {
                self.instructions[i] = Op::LocalLtEConstJump(Op::pack_pair(s, c), t);
                self.remove_instructions(i + 1, 3, &mut jump_targets);
                continue;
            }

            // Pattern: LoadLocal(s) + LoadConst(c) + CompareLt + PopJumpIfFalse(t)
            if let (
                Op::LoadLocal(s),
                Op::LoadConst(c),
                Op::CompareLt,
                Op::PopJumpIfFalse(t),
            ) = (
                self.instructions[i],
                self.instructions[i + 1],
                self.instructions[i + 2],
                self.instructions[i + 3],
            ) {
                self.instructions[i] = Op::LocalLtConstJump(Op::pack_pair(s, c), t);
                self.remove_instructions(i + 1, 3, &mut jump_targets);
                continue;
            }

            // Pattern: LoadLocal(s) + LoadConst(c) + CompareGt + PopJumpIfFalse(t)
            if let (
                Op::LoadLocal(s),
                Op::LoadConst(c),
                Op::CompareGt,
                Op::PopJumpIfFalse(t),
            ) = (
                self.instructions[i],
                self.instructions[i + 1],
                self.instructions[i + 2],
                self.instructions[i + 3],
            ) {
                self.instructions[i] = Op::LocalGtConstJump(Op::pack_pair(s, c), t);
                self.remove_instructions(i + 1, 3, &mut jump_targets);
                continue;
            }

            // Pattern: LoadGlobal(s) + LoadConst(c) + BinaryAdd + StoreGlobal(s)
            if let (
                Op::LoadGlobal(s1),
                Op::LoadConst(c),
                Op::BinaryAdd,
                Op::StoreGlobal(s2),
            ) = (
                self.instructions[i],
                self.instructions[i + 1],
                self.instructions[i + 2],
                self.instructions[i + 3],
            ) {
                if s1 == s2 {
                    if matches!(self.constants.get(c as usize), Some(Value::Integer(_))) {
                        self.instructions[i] = Op::IncrGlobalByConst(s1, c);
                        self.remove_instructions(i + 1, 3, &mut jump_targets);
                        continue;
                    }
                }
            }

            i += 1;
        }

        // 2-instruction fusions
        i = 0;
        while i + 1 < self.instructions.len() {
            if i + 1 < jump_targets.len() && jump_targets[i + 1] {
                i += 1;
                continue;
            }

            // Pattern: LoadLocal(a) + LoadLocal(b) → LoadLocalPair(a, b)
            if let (Op::LoadLocal(a), Op::LoadLocal(b)) = (
                self.instructions[i],
                self.instructions[i + 1],
            ) {
                self.instructions[i] = Op::LoadLocalPair(Op::pack_pair(a, b));
                self.remove_instructions(i + 1, 1, &mut jump_targets);
                continue;
            }

            i += 1;
        }
    }

    /// Remove `count` instructions starting at `start`, remapping all jump targets.
    fn remove_instructions(&mut self, start: usize, count: usize, jump_targets: &mut Vec<bool>) {
        self.instructions.drain(start..start + count);

        // Rebuild jump_targets
        jump_targets.clear();
        jump_targets.resize(self.instructions.len() + 1, false);

        // Remap jumps: any jump target > start needs to decrease by count
        for op in &mut self.instructions {
            match op {
                Op::Jump(ref mut t)
                | Op::PopJumpIfFalse(ref mut t)
                | Op::JumpIfTrueOrPop(ref mut t)
                | Op::JumpIfFalseOrPop(ref mut t)
                | Op::LocalLtLocalJump(_, ref mut t)
                | Op::LocalLtEConstJump(_, ref mut t)
                | Op::LocalLtConstJump(_, ref mut t)
                | Op::LocalGtConstJump(_, ref mut t) => {
                    if *t as usize >= start + count {
                        *t -= count as u32;
                    } else if *t as usize > start {
                        *t = start as u32;
                    }
                    let tv = *t as usize;
                    if tv < jump_targets.len() {
                        jump_targets[tv] = true;
                    }
                }
                _ => {}
            }
        }
    }
}

/// Compile a list of top-level statements into a CodeObject.
pub fn compile_module(stmts: &[ast::Stmt], interner: &mut Interner) -> CodeObject {
    let mut builder = CodeBuilder::new(false);
    compile_block(&mut builder, stmts, interner);
    builder.build()
}

/// Compile a function body into a CodeObject with local slots.
fn compile_function(
    params: &[Symbol],
    body: &[ast::Stmt],
    interner: &mut Interner,
) -> CodeObject {
    let mut builder = CodeBuilder::new(true);

    // Assign slots for params first
    for p in params {
        let slot = builder.num_locals;
        builder.local_slots.insert(*p, slot);
        builder.num_locals += 1;
    }

    // Scan body for local assignments
    scan_locals(body, &mut builder);

    compile_block(&mut builder, body, interner);

    // Ensure we always return
    builder.emit(Op::ReturnNone);

    builder.build()
}

fn scan_locals(stmts: &[ast::Stmt], builder: &mut CodeBuilder) {
    for stmt in stmts {
        match stmt {
            ast::Stmt::Assign { target, .. } | ast::Stmt::AugAssign { target, .. } => {
                if !builder.local_slots.contains_key(target) {
                    let slot = builder.num_locals;
                    builder.local_slots.insert(*target, slot);
                    builder.num_locals += 1;
                }
            }
            ast::Stmt::For { target, body, .. } => {
                if !builder.local_slots.contains_key(target) {
                    let slot = builder.num_locals;
                    builder.local_slots.insert(*target, slot);
                    builder.num_locals += 1;
                }
                scan_locals(body, builder);
            }
            ast::Stmt::If {
                body,
                elif_clauses,
                else_body,
                ..
            } => {
                scan_locals(body, builder);
                for (_, elif_body) in elif_clauses {
                    scan_locals(elif_body, builder);
                }
                if let Some(else_b) = else_body {
                    scan_locals(else_b, builder);
                }
            }
            ast::Stmt::While { body, .. } => {
                scan_locals(body, builder);
            }
            _ => {}
        }
    }
}

fn compile_block(builder: &mut CodeBuilder, stmts: &[ast::Stmt], interner: &mut Interner) {
    for stmt in stmts {
        compile_stmt(builder, stmt, interner);
    }
}

fn compile_stmt(builder: &mut CodeBuilder, stmt: &ast::Stmt, interner: &mut Interner) {
    match stmt {
        ast::Stmt::Expression(expr) => {
            compile_expr(builder, expr, interner);
            builder.emit(Op::Pop);
        }
        ast::Stmt::Assign { target, value } => {
            // Detect `x = x + "str"` or `x = x + [expr]` where x is global
            // → TakeGlobal for ownership transfer (avoids Rc clone for heap types)
            if let ast::Expr::BinaryOp { left, op: BinOp::Add, right } = value {
                if let ast::Expr::Identifier(name) = left.as_ref() {
                    if *name == *target && !builder.resolve_name(*name) {
                        let is_heap_rhs = matches!(right.as_ref(),
                            ast::Expr::StringLiteral(_) | ast::Expr::List(_));
                        if is_heap_rhs {
                            builder.emit(Op::TakeGlobal(*target));
                            compile_expr(builder, right, interner);
                            builder.emit(Op::BinaryAdd);
                            builder.emit_store(*target);
                            return;
                        }
                    }
                }
            }
            compile_expr(builder, value, interner);
            builder.emit_store(*target);
        }
        ast::Stmt::AugAssign { target, op, value } => {
            if matches!(op, BinOp::Add) {
                if let ast::Expr::List(elements) = value {
                    if elements.len() == 1 {
                        builder.emit_load(*target);
                        compile_expr(builder, &elements[0], interner);
                        builder.emit(Op::ListAppend);
                        builder.emit_store(*target);
                        return;
                    }
                }
            }
            builder.emit_load(*target);
            compile_expr(builder, value, interner);
            emit_binop(builder, op);
            builder.emit_store(*target);
        }
        ast::Stmt::Print(args) => {
            for arg in args {
                compile_expr(builder, arg, interner);
            }
            builder.emit(Op::Print(args.len() as u8));
        }
        ast::Stmt::If {
            condition,
            body,
            elif_clauses,
            else_body,
        } => {
            compile_expr(builder, condition, interner);
            let else_jump = builder.emit(Op::PopJumpIfFalse(0));

            compile_block(builder, body, interner);

            if elif_clauses.is_empty() && else_body.is_none() {
                let end = builder.current_offset();
                builder.patch_jump(else_jump, end);
            } else {
                let end_jump = builder.emit(Op::Jump(0));
                let elif_start = builder.current_offset();
                builder.patch_jump(else_jump, elif_start);

                let mut end_jumps = vec![end_jump];

                for (elif_cond, elif_body) in elif_clauses {
                    compile_expr(builder, elif_cond, interner);
                    let next_jump = builder.emit(Op::PopJumpIfFalse(0));
                    compile_block(builder, elif_body, interner);
                    end_jumps.push(builder.emit(Op::Jump(0)));
                    let next = builder.current_offset();
                    builder.patch_jump(next_jump, next);
                }

                if let Some(else_b) = else_body {
                    compile_block(builder, else_b, interner);
                }

                let end = builder.current_offset();
                for ej in end_jumps {
                    builder.patch_jump(ej, end);
                }
            }
        }
        ast::Stmt::While { condition, body } => {
            let loop_start = builder.current_offset();

            compile_expr(builder, condition, interner);
            let exit_jump = builder.emit(Op::PopJumpIfFalse(0));

            builder.loop_stack.push(LoopContext {
                continue_target: loop_start,
                break_patches: vec![],
            });

            compile_block(builder, body, interner);
            builder.emit(Op::Jump(loop_start as u32));

            let loop_end = builder.current_offset();
            builder.patch_jump(exit_jump, loop_end);

            let ctx = builder.loop_stack.pop().unwrap();
            for patch in ctx.break_patches {
                builder.patch_jump(patch, loop_end);
            }
        }
        ast::Stmt::For { target, iter, body } => {
            // Compile as:
            //   list = eval(iter)
            //   idx = 0
            //   loop: if idx >= len(list): goto end
            //         target = list[idx]
            //         <body>
            //         idx += 1
            //         goto loop
            //   end:
            let list_slot = builder.alloc_hidden_local();
            let idx_slot = builder.alloc_hidden_local();

            compile_expr(builder, iter, interner);
            builder.emit(Op::StoreLocal(list_slot));

            let zero = builder.add_const(Value::Integer(0));
            builder.emit(Op::LoadConst(zero));
            builder.emit(Op::StoreLocal(idx_slot));

            let loop_start = builder.current_offset();

            // idx < len(list)
            builder.emit(Op::LoadLocal(idx_slot));
            builder.emit(Op::LoadLocal(list_slot));
            builder.emit(Op::Len);
            builder.emit(Op::CompareLt);
            let exit_jump = builder.emit(Op::PopJumpIfFalse(0));

            // target = list[idx]
            builder.emit(Op::LoadLocal(list_slot));
            builder.emit(Op::LoadLocal(idx_slot));
            builder.emit(Op::BinarySubscript);
            builder.emit_store(*target);

            let continue_target = builder.current_offset();
            builder.loop_stack.push(LoopContext {
                continue_target,
                break_patches: vec![],
            });

            compile_block(builder, body, interner);

            // idx += 1
            let inc_start = builder.current_offset();
            builder.emit(Op::LoadLocal(idx_slot));
            let one = builder.add_const(Value::Integer(1));
            builder.emit(Op::LoadConst(one));
            builder.emit(Op::BinaryAdd);
            builder.emit(Op::StoreLocal(idx_slot));
            builder.emit(Op::Jump(loop_start as u32));

            let loop_end = builder.current_offset();
            builder.patch_jump(exit_jump, loop_end);

            // Fix continue to jump to idx increment, not condition check
            let ctx = builder.loop_stack.pop().unwrap();
            for patch in ctx.break_patches {
                builder.patch_jump(patch, loop_end);
            }
        }
        ast::Stmt::FunctionDef { name, params, body } => {
            let code = compile_function(params, body, interner);
            let param_slots: Vec<u16> = params.iter().map(|p| code.local_slots[p]).collect();
            let func_obj = FunctionObj {
                name: *name,
                params: params.clone(),
                param_slots,
                code: Rc::new(code),
            };
            let idx = builder.add_const(Value::Function(Rc::new(func_obj)));
            builder.emit(Op::LoadConst(idx));
            // Always store functions as globals
            builder.emit(Op::StoreGlobal(*name));
        }
        ast::Stmt::Return(expr) => {
            if let Some(e) = expr {
                compile_expr(builder, e, interner);
                builder.emit(Op::Return);
            } else {
                builder.emit(Op::ReturnNone);
            }
        }
        ast::Stmt::Break => {
            let patch = builder.emit(Op::Jump(0));
            builder.loop_stack.last_mut().unwrap().break_patches.push(patch);
        }
        ast::Stmt::Continue => {
            let target = builder.loop_stack.last().unwrap().continue_target;
            builder.emit(Op::Jump(target as u32));
        }
        ast::Stmt::Pass => {}
    }
}

fn compile_expr(builder: &mut CodeBuilder, expr: &ast::Expr, interner: &mut Interner) {
    match expr {
        ast::Expr::Integer(n) => {
            let idx = builder.add_const(Value::Integer(*n));
            builder.emit(Op::LoadConst(idx));
        }
        ast::Expr::Float(f) => {
            let idx = builder.add_const(Value::Float(*f));
            builder.emit(Op::LoadConst(idx));
        }
        ast::Expr::StringLiteral(s) => {
            let idx = builder.add_const(Value::String(Rc::new(s.clone())));
            builder.emit(Op::LoadConst(idx));
        }
        ast::Expr::Boolean(b) => {
            let idx = builder.add_const(Value::Boolean(*b));
            builder.emit(Op::LoadConst(idx));
        }
        ast::Expr::None => {
            let idx = builder.add_const(Value::None);
            builder.emit(Op::LoadConst(idx));
        }
        ast::Expr::Identifier(name) => {
            builder.emit_load(*name);
        }
        ast::Expr::BinaryOp { left, op, right } => {
            match op {
                ast::BinOp::And => {
                    compile_expr(builder, left, interner);
                    let jump = builder.emit(Op::JumpIfFalseOrPop(0));
                    compile_expr(builder, right, interner);
                    let end = builder.current_offset();
                    builder.patch_jump(jump, end);
                }
                ast::BinOp::Or => {
                    compile_expr(builder, left, interner);
                    let jump = builder.emit(Op::JumpIfTrueOrPop(0));
                    compile_expr(builder, right, interner);
                    let end = builder.current_offset();
                    builder.patch_jump(jump, end);
                }
                _ => {
                    compile_expr(builder, left, interner);
                    compile_expr(builder, right, interner);
                    emit_binop(builder, op);
                }
            }
        }
        ast::Expr::UnaryOp { op, operand } => {
            compile_expr(builder, operand, interner);
            match op {
                ast::UnaryOp::Neg => { builder.emit(Op::UnaryNeg); }
                ast::UnaryOp::Not => { builder.emit(Op::UnaryNot); }
            }
        }
        ast::Expr::Call { function, args } => {
            // If calling a global function, emit CallGlobal to skip stack push
            if let ast::Expr::Identifier(name) = function.as_ref() {
                if !builder.resolve_name(*name) {
                    // It's a global — compile args, then CallGlobal
                    for arg in args {
                        compile_expr(builder, arg, interner);
                    }
                    builder.emit(Op::CallGlobal(*name, args.len() as u8));
                    return;
                }
            }
            compile_expr(builder, function, interner);
            for arg in args {
                compile_expr(builder, arg, interner);
            }
            builder.emit(Op::Call(args.len() as u8));
        }
        ast::Expr::Index { object, index } => {
            compile_expr(builder, object, interner);
            compile_expr(builder, index, interner);
            builder.emit(Op::BinarySubscript);
        }
        ast::Expr::Attribute { object, name } => {
            compile_expr(builder, object, interner);
            builder.emit(Op::LoadAttr(*name));
        }
        ast::Expr::List(elements) => {
            for el in elements {
                compile_expr(builder, el, interner);
            }
            builder.emit(Op::BuildList(elements.len() as u16));
        }
        ast::Expr::Dict(pairs) => {
            for (k, v) in pairs {
                compile_expr(builder, k, interner);
                compile_expr(builder, v, interner);
            }
            builder.emit(Op::BuildDict(pairs.len() as u16));
        }
        ast::Expr::Compare {
            left,
            ops,
            comparators,
        } => {
            if ops.len() == 1 {
                compile_expr(builder, left, interner);
                compile_expr(builder, &comparators[0], interner);
                emit_cmpop(builder, &ops[0]);
            } else {
                // Chained: a op1 b op2 c → (a op1 b) and (b op2 c)
                compile_expr(builder, left, interner);
                compile_expr(builder, &comparators[0], interner);
                emit_cmpop(builder, &ops[0]);

                let mut end_jumps = Vec::new();
                for i in 1..ops.len() {
                    end_jumps.push(builder.emit(Op::JumpIfFalseOrPop(0)));
                    // Re-evaluate the previous comparator as the new left
                    compile_expr(builder, &comparators[i - 1], interner);
                    compile_expr(builder, &comparators[i], interner);
                    emit_cmpop(builder, &ops[i]);
                }
                let end = builder.current_offset();
                for ej in end_jumps {
                    builder.patch_jump(ej, end);
                }
            }
        }
    }
}

fn emit_binop(builder: &mut CodeBuilder, op: &ast::BinOp) {
    let instr = match op {
        ast::BinOp::Add => Op::BinaryAdd,
        ast::BinOp::Sub => Op::BinarySub,
        ast::BinOp::Mul => Op::BinaryMul,
        ast::BinOp::Div => Op::BinaryDiv,
        ast::BinOp::FloorDiv => Op::BinaryFloorDiv,
        ast::BinOp::Mod => Op::BinaryMod,
        ast::BinOp::Pow => Op::BinaryPow,
        ast::BinOp::And | ast::BinOp::Or => unreachable!("handled in compile_expr"),
    };
    builder.emit(instr);
}

fn emit_cmpop(builder: &mut CodeBuilder, op: &ast::CmpOp) {
    let instr = match op {
        ast::CmpOp::Eq => Op::CompareEq,
        ast::CmpOp::NotEq => Op::CompareNotEq,
        ast::CmpOp::Lt => Op::CompareLt,
        ast::CmpOp::LtE => Op::CompareLtE,
        ast::CmpOp::Gt => Op::CompareGt,
        ast::CmpOp::GtE => Op::CompareGtE,
        ast::CmpOp::In => Op::CompareIn,
        ast::CmpOp::NotIn => Op::CompareNotIn,
    };
    builder.emit(instr);
}
