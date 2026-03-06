use crate::ast::*;
use crate::symbol::{Interner, Symbol};
use crate::token::Token;

pub struct Parser<'a> {
    tokens: Vec<Token>,
    pos: usize,
    interner: &'a mut Interner,
}

impl<'a> Parser<'a> {
    pub fn new(tokens: Vec<Token>, interner: &'a mut Interner) -> Self {
        Parser {
            tokens,
            pos: 0,
            interner,
        }
    }

    fn intern(&mut self, name: &str) -> Symbol {
        self.interner.intern(name)
    }

    pub fn parse(&mut self) -> Result<Vec<Stmt>, String> {
        let mut stmts = Vec::new();
        self.skip_newlines();
        while !self.at_end() {
            stmts.push(self.parse_statement()?);
            self.skip_newlines();
        }
        Ok(stmts)
    }

    fn current(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    fn advance(&mut self) -> Token {
        let tok = self.current().clone();
        self.pos += 1;
        tok
    }

    fn expect(&mut self, expected: &Token) -> Result<(), String> {
        if self.current() == expected {
            self.advance();
            Ok(())
        } else {
            Err(format!(
                "Expected {:?}, got {:?} at position {}",
                expected,
                self.current(),
                self.pos
            ))
        }
    }

    fn at_end(&self) -> bool {
        matches!(self.current(), Token::Eof)
    }

    fn skip_newlines(&mut self) {
        while matches!(self.current(), Token::Newline) {
            self.advance();
        }
    }

    fn parse_statement(&mut self) -> Result<Stmt, String> {
        match self.current() {
            Token::Def => self.parse_function_def(),
            Token::If => self.parse_if(),
            Token::While => self.parse_while(),
            Token::For => self.parse_for(),
            Token::Return => self.parse_return(),
            Token::Break => {
                self.advance();
                Ok(Stmt::Break)
            }
            Token::Continue => {
                self.advance();
                Ok(Stmt::Continue)
            }
            Token::Pass => {
                self.advance();
                Ok(Stmt::Pass)
            }
            Token::Print => self.parse_print(),
            _ => self.parse_expr_or_assign(),
        }
    }

    fn parse_function_def(&mut self) -> Result<Stmt, String> {
        self.expect(&Token::Def)?;
        let name = match self.advance() {
            Token::Identifier(n) => self.intern(&n),
            t => return Err(format!("Expected function name, got {:?}", t)),
        };
        self.expect(&Token::LeftParen)?;

        let mut params = Vec::new();
        while *self.current() != Token::RightParen {
            match self.advance() {
                Token::Identifier(p) => params.push(self.intern(&p)),
                t => return Err(format!("Expected parameter name, got {:?}", t)),
            }
            if *self.current() == Token::Comma {
                self.advance();
            }
        }
        self.expect(&Token::RightParen)?;
        self.expect(&Token::Colon)?;

        let body = self.parse_block()?;

        Ok(Stmt::FunctionDef { name, params, body })
    }

    fn parse_block(&mut self) -> Result<Vec<Stmt>, String> {
        self.skip_newlines();
        self.expect(&Token::Indent)?;
        let mut stmts = Vec::new();
        self.skip_newlines();

        while !matches!(self.current(), Token::Dedent | Token::Eof) {
            stmts.push(self.parse_statement()?);
            self.skip_newlines();
        }

        if *self.current() == Token::Dedent {
            self.advance();
        }

        Ok(stmts)
    }

    fn parse_if(&mut self) -> Result<Stmt, String> {
        self.expect(&Token::If)?;
        let condition = self.parse_expression()?;
        self.expect(&Token::Colon)?;
        let body = self.parse_block()?;

        let mut elif_clauses = Vec::new();
        while *self.current() == Token::Elif {
            self.advance();
            let elif_cond = self.parse_expression()?;
            self.expect(&Token::Colon)?;
            let elif_body = self.parse_block()?;
            elif_clauses.push((elif_cond, elif_body));
        }

        let else_body = if *self.current() == Token::Else {
            self.advance();
            self.expect(&Token::Colon)?;
            Some(self.parse_block()?)
        } else {
            None
        };

        Ok(Stmt::If {
            condition,
            body,
            elif_clauses,
            else_body,
        })
    }

    fn parse_while(&mut self) -> Result<Stmt, String> {
        self.expect(&Token::While)?;
        let condition = self.parse_expression()?;
        self.expect(&Token::Colon)?;
        let body = self.parse_block()?;
        Ok(Stmt::While { condition, body })
    }

    fn parse_for(&mut self) -> Result<Stmt, String> {
        self.expect(&Token::For)?;
        let target = match self.advance() {
            Token::Identifier(n) => self.intern(&n),
            t => return Err(format!("Expected variable name, got {:?}", t)),
        };
        self.expect(&Token::In)?;
        let iter = self.parse_expression()?;
        self.expect(&Token::Colon)?;
        let body = self.parse_block()?;
        Ok(Stmt::For { target, iter, body })
    }

    fn parse_return(&mut self) -> Result<Stmt, String> {
        self.expect(&Token::Return)?;
        if matches!(self.current(), Token::Newline | Token::Eof | Token::Dedent) {
            Ok(Stmt::Return(None))
        } else {
            Ok(Stmt::Return(Some(self.parse_expression()?)))
        }
    }

    fn parse_print(&mut self) -> Result<Stmt, String> {
        self.expect(&Token::Print)?;
        self.expect(&Token::LeftParen)?;
        let mut args = Vec::new();
        while *self.current() != Token::RightParen {
            args.push(self.parse_expression()?);
            if *self.current() == Token::Comma {
                self.advance();
            }
        }
        self.expect(&Token::RightParen)?;
        Ok(Stmt::Print(args))
    }

    fn parse_expr_or_assign(&mut self) -> Result<Stmt, String> {
        let expr = self.parse_expression()?;

        match self.current() {
            Token::Assign => {
                self.advance();
                if let Expr::Identifier(name) = expr {
                    let value = self.parse_expression()?;
                    Ok(Stmt::Assign { target: name, value })
                } else {
                    Err("Invalid assignment target".to_string())
                }
            }
            Token::PlusAssign | Token::MinusAssign | Token::StarAssign | Token::SlashAssign => {
                let op = match self.advance() {
                    Token::PlusAssign => BinOp::Add,
                    Token::MinusAssign => BinOp::Sub,
                    Token::StarAssign => BinOp::Mul,
                    Token::SlashAssign => BinOp::Div,
                    _ => unreachable!(),
                };
                if let Expr::Identifier(name) = expr {
                    let value = self.parse_expression()?;
                    Ok(Stmt::AugAssign {
                        target: name,
                        op,
                        value,
                    })
                } else {
                    Err("Invalid augmented assignment target".to_string())
                }
            }
            _ => Ok(Stmt::Expression(expr)),
        }
    }

    fn parse_expression(&mut self) -> Result<Expr, String> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_and()?;
        while *self.current() == Token::Or {
            self.advance();
            let right = self.parse_and()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinOp::Or,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_not()?;
        while *self.current() == Token::And {
            self.advance();
            let right = self.parse_not()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinOp::And,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<Expr, String> {
        if *self.current() == Token::Not {
            self.advance();
            let operand = self.parse_not()?;
            Ok(Expr::UnaryOp {
                op: UnaryOp::Not,
                operand: Box::new(operand),
            })
        } else {
            self.parse_comparison()
        }
    }

    fn parse_comparison(&mut self) -> Result<Expr, String> {
        let left = self.parse_addition()?;

        let mut ops = Vec::new();
        let mut comparators = Vec::new();

        loop {
            let op = match self.current() {
                Token::Equal => CmpOp::Eq,
                Token::NotEqual => CmpOp::NotEq,
                Token::Less => CmpOp::Lt,
                Token::LessEqual => CmpOp::LtE,
                Token::Greater => CmpOp::Gt,
                Token::GreaterEqual => CmpOp::GtE,
                Token::In => CmpOp::In,
                _ => break,
            };
            self.advance();
            ops.push(op);
            comparators.push(self.parse_addition()?);
        }

        if ops.is_empty() {
            Ok(left)
        } else {
            Ok(Expr::Compare {
                left: Box::new(left),
                ops,
                comparators,
            })
        }
    }

    fn parse_addition(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_multiplication()?;
        loop {
            let op = match self.current() {
                Token::Plus => BinOp::Add,
                Token::Minus => BinOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.parse_multiplication()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_multiplication(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_power()?;
        loop {
            let op = match self.current() {
                Token::Star => BinOp::Mul,
                Token::Slash => BinOp::Div,
                Token::DoubleSlash => BinOp::FloorDiv,
                Token::Percent => BinOp::Mod,
                _ => break,
            };
            self.advance();
            let right = self.parse_power()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_power(&mut self) -> Result<Expr, String> {
        let base = self.parse_unary()?;
        if *self.current() == Token::DoubleStar {
            self.advance();
            let exp = self.parse_unary()?;
            Ok(Expr::BinaryOp {
                left: Box::new(base),
                op: BinOp::Pow,
                right: Box::new(exp),
            })
        } else {
            Ok(base)
        }
    }

    fn parse_unary(&mut self) -> Result<Expr, String> {
        if *self.current() == Token::Minus {
            self.advance();
            let operand = self.parse_postfix()?;
            Ok(Expr::UnaryOp {
                op: UnaryOp::Neg,
                operand: Box::new(operand),
            })
        } else {
            self.parse_postfix()
        }
    }

    fn parse_postfix(&mut self) -> Result<Expr, String> {
        let mut expr = self.parse_primary()?;

        loop {
            match self.current() {
                Token::LeftParen => {
                    self.advance();
                    let mut args = Vec::new();
                    while *self.current() != Token::RightParen {
                        args.push(self.parse_expression()?);
                        if *self.current() == Token::Comma {
                            self.advance();
                        }
                    }
                    self.expect(&Token::RightParen)?;
                    expr = Expr::Call {
                        function: Box::new(expr),
                        args,
                    };
                }
                Token::LeftBracket => {
                    self.advance();
                    let index = self.parse_expression()?;
                    self.expect(&Token::RightBracket)?;
                    expr = Expr::Index {
                        object: Box::new(expr),
                        index: Box::new(index),
                    };
                }
                Token::Dot => {
                    self.advance();
                    let name = match self.advance() {
                        Token::Identifier(n) => self.intern(&n),
                        t => return Err(format!("Expected attribute name, got {:?}", t)),
                    };
                    expr = Expr::Attribute {
                        object: Box::new(expr),
                        name,
                    };
                }
                _ => break,
            }
        }

        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, String> {
        match self.advance() {
            Token::Integer(n) => Ok(Expr::Integer(n)),
            Token::Float(f) => Ok(Expr::Float(f)),
            Token::StringLiteral(s) => Ok(Expr::StringLiteral(s)),
            Token::True => Ok(Expr::Boolean(true)),
            Token::False => Ok(Expr::Boolean(false)),
            Token::None => Ok(Expr::None),
            Token::Identifier(name) => Ok(Expr::Identifier(self.intern(&name))),
            Token::LeftParen => {
                let expr = self.parse_expression()?;
                self.expect(&Token::RightParen)?;
                Ok(expr)
            }
            Token::LeftBracket => {
                let mut elements = Vec::new();
                while *self.current() != Token::RightBracket {
                    elements.push(self.parse_expression()?);
                    if *self.current() == Token::Comma {
                        self.advance();
                    }
                }
                self.expect(&Token::RightBracket)?;
                Ok(Expr::List(elements))
            }
            Token::LeftBrace => {
                let mut pairs = Vec::new();
                while *self.current() != Token::RightBrace {
                    let key = self.parse_expression()?;
                    self.expect(&Token::Colon)?;
                    let value = self.parse_expression()?;
                    pairs.push((key, value));
                    if *self.current() == Token::Comma {
                        self.advance();
                    }
                }
                self.expect(&Token::RightBrace)?;
                Ok(Expr::Dict(pairs))
            }
            t => Err(format!("Unexpected token: {:?}", t)),
        }
    }
}
