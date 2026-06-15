use crate::ast::{BinaryOp, Expr, Program, Stmt, UnaryOp};
use crate::error::{JsError, JsResult, Span};
use crate::token::{Token, TokenKind};

pub fn parse(tokens: Vec<Token>) -> JsResult<Program> {
    Parser { tokens, pos: 0 }.program()
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn program(&mut self) -> JsResult<Program> {
        let mut statements = Vec::new();
        while !self.at(&TokenKind::Eof) {
            statements.push(self.statement()?);
        }
        Ok(Program { statements })
    }

    fn statement(&mut self) -> JsResult<Stmt> {
        if self.eat(&TokenKind::Let) || self.eat(&TokenKind::Var) {
            self.var_decl(true)
        } else if self.eat(&TokenKind::Const) {
            self.var_decl(false)
        } else if self.eat(&TokenKind::Function) {
            self.function_decl()
        } else if self.eat(&TokenKind::Return) {
            self.return_stmt()
        } else if self.eat(&TokenKind::Throw) {
            self.throw_stmt()
        } else if self.eat(&TokenKind::Try) {
            self.try_stmt()
        } else if self.eat(&TokenKind::Break) {
            self.optional_semicolon();
            Ok(Stmt::Break)
        } else if self.eat(&TokenKind::If) {
            self.if_stmt()
        } else if self.eat(&TokenKind::While) {
            self.while_stmt()
        } else if self.eat(&TokenKind::For) {
            self.for_stmt()
        } else if self.eat(&TokenKind::Switch) {
            self.switch_stmt()
        } else if self.eat(&TokenKind::LeftBrace) {
            Ok(Stmt::Block(self.block()?))
        } else {
            let e = self.expression()?;
            self.optional_semicolon();
            Ok(Stmt::Expr(e))
        }
    }

    fn var_decl(&mut self, mutable: bool) -> JsResult<Stmt> {
        let mut declarations = Vec::new();
        loop {
            let name = self.identifier()?;
            let value = if self.eat(&TokenKind::Assign) {
                self.expression()?
            } else {
                Expr::Undefined
            };
            declarations.push((name, value));
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.optional_semicolon();
        if declarations.len() == 1 {
            let (name, value) = declarations.remove(0);
            Ok(Stmt::VarDecl {
                name,
                value,
                mutable,
            })
        } else {
            Ok(Stmt::VarDecls {
                declarations,
                mutable,
            })
        }
    }

    fn function_decl(&mut self) -> JsResult<Stmt> {
        let name = self.identifier()?;
        self.expect(&TokenKind::LeftParen)?;
        let params = self.params()?;
        self.expect(&TokenKind::LeftBrace)?;
        Ok(Stmt::FunctionDecl {
            name,
            params,
            body: self.block()?,
        })
    }

    fn return_stmt(&mut self) -> JsResult<Stmt> {
        if self.eat(&TokenKind::Semicolon) {
            return Ok(Stmt::Return(None));
        }
        let value = if self.at(&TokenKind::RightBrace) {
            None
        } else {
            Some(self.expression()?)
        };
        self.optional_semicolon();
        Ok(Stmt::Return(value))
    }

    fn throw_stmt(&mut self) -> JsResult<Stmt> {
        let value = self.expression()?;
        self.optional_semicolon();
        Ok(Stmt::Throw(value))
    }

    fn try_stmt(&mut self) -> JsResult<Stmt> {
        self.expect(&TokenKind::LeftBrace)?;
        let block = self.block()?;
        let mut catch_param = None;
        let mut catch_block = None;
        let mut finally_block = None;
        if self.eat(&TokenKind::Catch) {
            self.expect(&TokenKind::LeftParen)?;
            catch_param = Some(self.identifier()?);
            self.expect(&TokenKind::RightParen)?;
            self.expect(&TokenKind::LeftBrace)?;
            catch_block = Some(self.block()?);
        }
        if self.eat(&TokenKind::Finally) {
            self.expect(&TokenKind::LeftBrace)?;
            finally_block = Some(self.block()?);
        }
        if catch_block.is_none() && finally_block.is_none() {
            return Err(self.error("try requires catch or finally"));
        }
        Ok(Stmt::Try {
            block,
            catch_param,
            catch_block,
            finally_block,
        })
    }

    fn if_stmt(&mut self) -> JsResult<Stmt> {
        self.expect(&TokenKind::LeftParen)?;
        let condition = self.expression()?;
        self.expect(&TokenKind::RightParen)?;
        let then_branch = self.statement_as_block()?;
        let else_branch = if self.eat(&TokenKind::Else) {
            self.statement_as_block()?
        } else {
            Vec::new()
        };
        Ok(Stmt::If {
            condition,
            then_branch,
            else_branch,
        })
    }

    fn while_stmt(&mut self) -> JsResult<Stmt> {
        self.expect(&TokenKind::LeftParen)?;
        let condition = self.expression()?;
        self.expect(&TokenKind::RightParen)?;
        Ok(Stmt::While {
            condition,
            body: self.statement_as_block()?,
        })
    }

    fn for_stmt(&mut self) -> JsResult<Stmt> {
        self.expect(&TokenKind::LeftParen)?;
        let init = if self.eat(&TokenKind::Semicolon) {
            None
        } else if self.eat(&TokenKind::Let) || self.eat(&TokenKind::Var) {
            Some(Box::new(self.var_decl(true)?))
        } else if self.eat(&TokenKind::Const) {
            Some(Box::new(self.var_decl(false)?))
        } else {
            let expr = self.expression()?;
            self.expect(&TokenKind::Semicolon)?;
            Some(Box::new(Stmt::Expr(expr)))
        };
        let condition = if self.eat(&TokenKind::Semicolon) {
            None
        } else {
            let expr = self.expression()?;
            self.expect(&TokenKind::Semicolon)?;
            Some(expr)
        };
        let update = if self.eat(&TokenKind::RightParen) {
            None
        } else {
            let expr = self.expression()?;
            self.expect(&TokenKind::RightParen)?;
            Some(expr)
        };
        Ok(Stmt::For {
            init,
            condition,
            update,
            body: self.statement_as_block()?,
        })
    }

    fn switch_stmt(&mut self) -> JsResult<Stmt> {
        self.expect(&TokenKind::LeftParen)?;
        let discriminant = self.expression()?;
        self.expect(&TokenKind::RightParen)?;
        self.expect(&TokenKind::LeftBrace)?;
        let mut cases = Vec::new();
        let mut default = Vec::new();
        while !self.at(&TokenKind::RightBrace) && !self.at(&TokenKind::Eof) {
            if self.eat(&TokenKind::Case) {
                let test = self.expression()?;
                self.expect(&TokenKind::Colon)?;
                let mut body = Vec::new();
                while !self.at(&TokenKind::Case)
                    && !self.at(&TokenKind::Default)
                    && !self.at(&TokenKind::RightBrace)
                    && !self.at(&TokenKind::Eof)
                {
                    body.push(self.statement()?);
                }
                cases.push((test, body));
            } else if self.eat(&TokenKind::Default) {
                self.expect(&TokenKind::Colon)?;
                while !self.at(&TokenKind::Case)
                    && !self.at(&TokenKind::RightBrace)
                    && !self.at(&TokenKind::Eof)
                {
                    default.push(self.statement()?);
                }
            } else {
                return Err(self.error("expected case or default"));
            }
        }
        self.expect(&TokenKind::RightBrace)?;
        Ok(Stmt::Switch {
            discriminant,
            cases,
            default,
        })
    }

    fn statement_as_block(&mut self) -> JsResult<Vec<Stmt>> {
        if self.eat(&TokenKind::LeftBrace) {
            self.block()
        } else {
            Ok(vec![self.statement()?])
        }
    }

    fn block(&mut self) -> JsResult<Vec<Stmt>> {
        let mut stmts = Vec::new();
        while !self.at(&TokenKind::RightBrace) && !self.at(&TokenKind::Eof) {
            stmts.push(self.statement()?);
        }
        self.expect(&TokenKind::RightBrace)?;
        Ok(stmts)
    }

    fn params(&mut self) -> JsResult<Vec<String>> {
        let mut params = Vec::new();
        if self.eat(&TokenKind::RightParen) {
            return Ok(params);
        }
        loop {
            params.push(self.identifier()?);
            if self.eat(&TokenKind::RightParen) {
                break;
            }
            self.expect(&TokenKind::Comma)?;
        }
        Ok(params)
    }

    fn expression(&mut self) -> JsResult<Expr> {
        self.assignment()
    }

    fn assignment(&mut self) -> JsResult<Expr> {
        let expr = self.conditional()?;
        if self.eat(&TokenKind::Assign) {
            if Self::is_assignable(&expr) {
                return Ok(Expr::Assign {
                    target: Box::new(expr),
                    value: Box::new(self.assignment()?),
                });
            }
            return Err(self.error("left side of assignment must be assignable"));
        }
        let op = if self.eat(&TokenKind::PlusAssign) {
            Some(BinaryOp::Add)
        } else if self.eat(&TokenKind::MinusAssign) {
            Some(BinaryOp::Subtract)
        } else if self.eat(&TokenKind::StarAssign) {
            Some(BinaryOp::Multiply)
        } else if self.eat(&TokenKind::SlashAssign) {
            Some(BinaryOp::Divide)
        } else if self.eat(&TokenKind::PercentAssign) {
            Some(BinaryOp::Remainder)
        } else {
            None
        };
        if let Some(op) = op {
            if Self::is_assignable(&expr) {
                return Ok(Expr::CompoundAssign {
                    target: Box::new(expr),
                    op,
                    value: Box::new(self.assignment()?),
                });
            }
            return Err(self.error("left side of assignment must be assignable"));
        }
        Ok(expr)
    }

    fn conditional(&mut self) -> JsResult<Expr> {
        let condition = self.logical_or()?;
        if self.eat(&TokenKind::Question) {
            let then_expr = self.expression()?;
            self.expect(&TokenKind::Colon)?;
            let else_expr = self.assignment()?;
            Ok(Expr::Conditional {
                condition: Box::new(condition),
                then_expr: Box::new(then_expr),
                else_expr: Box::new(else_expr),
            })
        } else {
            Ok(condition)
        }
    }

    fn logical_or(&mut self) -> JsResult<Expr> {
        self.binary(Self::logical_and, &[(TokenKind::Or, BinaryOp::Or)])
    }
    fn logical_and(&mut self) -> JsResult<Expr> {
        self.binary(Self::equality, &[(TokenKind::And, BinaryOp::And)])
    }

    fn equality(&mut self) -> JsResult<Expr> {
        self.binary(
            Self::comparison,
            &[
                (TokenKind::Equal, BinaryOp::Equal),
                (TokenKind::NotEqual, BinaryOp::NotEqual),
                (TokenKind::StrictEqual, BinaryOp::StrictEqual),
                (TokenKind::StrictNotEqual, BinaryOp::StrictNotEqual),
            ],
        )
    }

    fn comparison(&mut self) -> JsResult<Expr> {
        self.binary(
            Self::term,
            &[
                (TokenKind::Less, BinaryOp::Less),
                (TokenKind::LessEqual, BinaryOp::LessEqual),
                (TokenKind::Greater, BinaryOp::Greater),
                (TokenKind::GreaterEqual, BinaryOp::GreaterEqual),
                (TokenKind::Instanceof, BinaryOp::Instanceof),
            ],
        )
    }

    fn term(&mut self) -> JsResult<Expr> {
        self.binary(
            Self::factor,
            &[
                (TokenKind::Plus, BinaryOp::Add),
                (TokenKind::Minus, BinaryOp::Subtract),
            ],
        )
    }

    fn factor(&mut self) -> JsResult<Expr> {
        self.binary(
            Self::unary,
            &[
                (TokenKind::Star, BinaryOp::Multiply),
                (TokenKind::Slash, BinaryOp::Divide),
                (TokenKind::Percent, BinaryOp::Remainder),
            ],
        )
    }

    fn binary(
        &mut self,
        next: fn(&mut Self) -> JsResult<Expr>,
        ops: &[(TokenKind, BinaryOp)],
    ) -> JsResult<Expr> {
        let mut expr = next(self)?;
        loop {
            let op = ops.iter().find(|(k, _)| self.at(k)).map(|(_, o)| *o);
            if let Some(op) = op {
                self.pos += 1;
                expr = Expr::Binary {
                    left: Box::new(expr),
                    op,
                    right: Box::new(next(self)?),
                };
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn unary(&mut self) -> JsResult<Expr> {
        if self.eat(&TokenKind::Bang) {
            Ok(Expr::Unary {
                op: UnaryOp::Not,
                expr: Box::new(self.unary()?),
            })
        } else if self.eat(&TokenKind::Minus) {
            Ok(Expr::Unary {
                op: UnaryOp::Negate,
                expr: Box::new(self.unary()?),
            })
        } else if self.eat(&TokenKind::Typeof) {
            Ok(Expr::Typeof(Box::new(self.unary()?)))
        } else if self.eat(&TokenKind::New) {
            let callee = self.new_callee()?;
            let args = if self.eat(&TokenKind::LeftParen) {
                self.arguments()?
            } else {
                Vec::new()
            };
            Ok(Expr::New {
                callee: Box::new(callee),
                args,
            })
        } else if self.eat(&TokenKind::PlusPlus) {
            let target = self.unary()?;
            if Self::is_assignable(&target) {
                Ok(Expr::Update {
                    target: Box::new(target),
                    delta: 1.0,
                    prefix: true,
                })
            } else {
                Err(self.error("increment target must be assignable"))
            }
        } else if self.eat(&TokenKind::MinusMinus) {
            let target = self.unary()?;
            if Self::is_assignable(&target) {
                Ok(Expr::Update {
                    target: Box::new(target),
                    delta: -1.0,
                    prefix: true,
                })
            } else {
                Err(self.error("decrement target must be assignable"))
            }
        } else {
            self.call()
        }
    }

    fn new_callee(&mut self) -> JsResult<Expr> {
        let mut expr = self.primary()?;
        loop {
            if self.eat(&TokenKind::LeftBracket) {
                let index = self.expression()?;
                self.expect(&TokenKind::RightBracket)?;
                expr = Expr::Index {
                    object: Box::new(expr),
                    index: Box::new(index),
                };
            } else if self.eat(&TokenKind::Dot) {
                let property = self.identifier()?;
                expr = Expr::Member {
                    object: Box::new(expr),
                    property,
                };
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn call(&mut self) -> JsResult<Expr> {
        let mut expr = self.primary()?;
        loop {
            if self.eat(&TokenKind::LeftParen) {
                expr = Expr::Call {
                    callee: Box::new(expr),
                    args: self.arguments()?,
                };
            } else if self.eat(&TokenKind::LeftBracket) {
                let index = self.expression()?;
                self.expect(&TokenKind::RightBracket)?;
                expr = Expr::Index {
                    object: Box::new(expr),
                    index: Box::new(index),
                };
            } else if self.eat(&TokenKind::Dot) {
                let property = self.identifier()?;
                expr = Expr::Member {
                    object: Box::new(expr),
                    property,
                };
            } else if self.eat(&TokenKind::PlusPlus) {
                if Self::is_assignable(&expr) {
                    expr = Expr::Update {
                        target: Box::new(expr),
                        delta: 1.0,
                        prefix: false,
                    };
                } else {
                    return Err(self.error("increment target must be assignable"));
                }
            } else if self.eat(&TokenKind::MinusMinus) {
                if Self::is_assignable(&expr) {
                    expr = Expr::Update {
                        target: Box::new(expr),
                        delta: -1.0,
                        prefix: false,
                    };
                } else {
                    return Err(self.error("decrement target must be assignable"));
                }
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn arguments(&mut self) -> JsResult<Vec<Expr>> {
        let mut args = Vec::new();
        if self.eat(&TokenKind::RightParen) {
            return Ok(args);
        }
        loop {
            args.push(self.expression()?);
            if self.eat(&TokenKind::RightParen) {
                break;
            }
            self.expect(&TokenKind::Comma)?;
        }
        Ok(args)
    }

    fn primary(&mut self) -> JsResult<Expr> {
        let token = self.advance().clone();
        match token.kind {
            TokenKind::Number(n) => Ok(Expr::Number(n)),
            TokenKind::String(s) => Ok(Expr::String(s)),
            TokenKind::True => Ok(Expr::Bool(true)),
            TokenKind::False => Ok(Expr::Bool(false)),
            TokenKind::Null => Ok(Expr::Null),
            TokenKind::Undefined => Ok(Expr::Undefined),
            TokenKind::This => Ok(Expr::This),
            TokenKind::Identifier(s) => Ok(Expr::Identifier(s)),
            TokenKind::Function => self.function_expr(),
            TokenKind::LeftParen => {
                let e = self.expression()?;
                self.expect(&TokenKind::RightParen)?;
                Ok(e)
            }
            TokenKind::LeftBracket => self.array_literal(),
            TokenKind::LeftBrace => self.object_literal(),
            _ => Err(JsError::parse("expected expression", token.span)),
        }
    }

    fn function_expr(&mut self) -> JsResult<Expr> {
        self.expect(&TokenKind::LeftParen)?;
        let params = self.params()?;
        self.expect(&TokenKind::LeftBrace)?;
        Ok(Expr::Function {
            params,
            body: self.block()?,
        })
    }

    fn array_literal(&mut self) -> JsResult<Expr> {
        let mut items = Vec::new();
        if self.eat(&TokenKind::RightBracket) {
            return Ok(Expr::Array(items));
        }
        loop {
            items.push(self.expression()?);
            if self.eat(&TokenKind::RightBracket) {
                break;
            }
            self.expect(&TokenKind::Comma)?;
            if self.eat(&TokenKind::RightBracket) {
                break;
            }
        }
        Ok(Expr::Array(items))
    }

    fn object_literal(&mut self) -> JsResult<Expr> {
        let mut props = Vec::new();
        if self.eat(&TokenKind::RightBrace) {
            return Ok(Expr::Object(props));
        }
        loop {
            let key = match self.advance().clone().kind {
                TokenKind::Identifier(s) | TokenKind::String(s) => s,
                TokenKind::Number(n) => n.to_string(),
                _ => return Err(self.error("expected object property name")),
            };
            self.expect(&TokenKind::Colon)?;
            let value = self.expression()?;
            props.push((key, value));
            if self.eat(&TokenKind::RightBrace) {
                break;
            }
            self.expect(&TokenKind::Comma)?;
            if self.eat(&TokenKind::RightBrace) {
                break;
            }
        }
        Ok(Expr::Object(props))
    }

    fn is_assignable(expr: &Expr) -> bool {
        matches!(
            expr,
            Expr::Identifier(_) | Expr::Member { .. } | Expr::Index { .. }
        )
    }

    fn identifier(&mut self) -> JsResult<String> {
        let token = self.advance().clone();
        if let TokenKind::Identifier(name) = token.kind {
            Ok(name)
        } else {
            Err(JsError::parse("expected identifier", token.span))
        }
    }

    fn optional_semicolon(&mut self) {
        self.eat(&TokenKind::Semicolon);
    }

    fn eat(&mut self, kind: &TokenKind) -> bool {
        if self.at(kind) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, kind: &TokenKind) -> JsResult<()> {
        if self.eat(kind) {
            Ok(())
        } else {
            Err(self.error(format!("expected {:?}", kind)))
        }
    }

    fn at(&self, kind: &TokenKind) -> bool {
        std::mem::discriminant(&self.current().kind) == std::mem::discriminant(kind)
    }

    fn current(&self) -> &Token {
        self.tokens
            .get(self.pos)
            .unwrap_or_else(|| self.tokens.last().expect("lexer emits eof"))
    }

    fn advance(&mut self) -> &Token {
        let pos = self.pos;
        self.pos += 1;
        self.tokens
            .get(pos)
            .unwrap_or_else(|| self.tokens.last().expect("lexer emits eof"))
    }

    fn error(&self, message: impl Into<String>) -> JsError {
        let span: Span = self.current().span;
        JsError::parse(message, span)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;
    #[test]
    fn parses_precedence() {
        let p = parse(lex("1 + 2 * 3;").unwrap()).unwrap();
        assert_eq!(p.statements.len(), 1);
    }
    #[test]
    fn parses_function() {
        assert!(parse(lex("function f(x){ return x; } f(1);").unwrap()).is_ok());
    }
}
