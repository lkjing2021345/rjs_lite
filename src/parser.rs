use crate::ast::{BinaryOp, ClassElement, Expr, ObjectPatternEntry, Pattern, Program, Stmt, UnaryOp};
use crate::error::{JsError, JsResult, Span};
use crate::token::{Token, TokenKind};

pub fn parse(tokens: Vec<Token>) -> JsResult<Program> {
    Parser {
        tokens,
        pos: 0,
        strict: false,
    }
    .program()
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    /// Program-level strict mode, enabled by a leading `"use strict"`
    /// directive. This affects how function declarations in statement
    /// positions are validated.
    strict: bool,
}

impl Parser {
    fn program(&mut self) -> JsResult<Program> {
        let mut statements = Vec::new();
        while !self.at(&TokenKind::Eof) {
            let statement = self.statement()?;
            // A leading `"use strict"` directive puts the whole program in
            // strict mode; mirror the interpreter's `detect_strict_mode`.
            if statements.is_empty() {
                if let Stmt::Expr(Expr::String(s)) = &statement {
                    if s == "use strict" {
                        self.strict = true;
                    }
                }
            }
            statements.push(statement);
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
        } else if self.eat(&TokenKind::Class) {
            self.class_decl()
        } else if self.eat(&TokenKind::Async) {
            self.async_function_decl()
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
        } else if self.eat(&TokenKind::Continue) {
            self.optional_semicolon();
            Ok(Stmt::Continue)
        } else if self.eat(&TokenKind::LeftBrace) {
            Ok(Stmt::Block(self.block()?))
        } else {
            let e = self.expression()?;
            // ASI rule (ES 12.9.1): a semicolon is inserted before an
            // offending token only when it is separated from the previous
            // token by at least one LineTerminator (or is `}`/EOF). If the
            // next token sits on the same line and cannot continue the
            // expression, no semicolon may be inserted and this is a
            // SyntaxError (e.g. `{1 2} 3`, `x = 1 else`).
            if !self.at(&TokenKind::Semicolon)
                && !self.at(&TokenKind::RightBrace)
                && !self.at(&TokenKind::Eof)
                && self.current().span.line == self.previous_span().line
            {
                return Err(self.error(
                    "missing semicolon before token on the same line",
                ));
            }
            self.optional_semicolon();
            Ok(Stmt::Expr(e))
        }
    }

    fn var_decl(&mut self, mutable: bool) -> JsResult<Stmt> {
        let mut declarations = Vec::new();
        loop {
            let name = self.pattern()?;
            let value = if self.eat(&TokenKind::Assign) {
                self.expression()?
            } else if mutable {
                Expr::Undefined
            } else {
                return Err(self.error("const declarations must be initialized"));
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
        // Optional generator marker: `function* name(...) { ... }`.
        let generator = self.eat(&TokenKind::Star);
        let name = self.identifier()?;
        self.expect(&TokenKind::LeftParen)?;
        let params = self.params()?;
        self.expect(&TokenKind::LeftBrace)?;
        let body = self.block()?;
        // Allow an optional trailing semicolon after the function body.
        self.optional_semicolon();
        Ok(Stmt::FunctionDecl {
            name,
            params,
            body,
            generator,
        })
    }

    fn async_function_decl(&mut self) -> JsResult<Stmt> {
        // `async function [*] name(...) { ... }`
        self.expect(&TokenKind::Function)?;
        let generator = self.eat(&TokenKind::Star);
        let name = self.identifier()?;
        self.expect(&TokenKind::LeftParen)?;
        let params = self.params()?;
        self.expect(&TokenKind::LeftBrace)?;
        let body = self.block()?;
        self.optional_semicolon();
        Ok(Stmt::FunctionDecl {
            name,
            params,
            body,
            generator,
        })
    }

    /// Parse a class declaration. `class` has already been consumed.
    fn class_decl(&mut self) -> JsResult<Stmt> {
        let name = self.identifier()?;
        let extends = self.class_extends()?;
        self.expect(&TokenKind::LeftBrace)?;
        let body = self.class_body()?;
        Ok(Stmt::ClassDecl { name, extends, body })
    }

    /// Parse a class expression. `class` has already been consumed.
    fn class_expr(&mut self) -> JsResult<Expr> {
        let name = self.optional_identifier()?;
        let extends = self.class_extends()?;
        self.expect(&TokenKind::LeftBrace)?;
        let body = self.class_body()?;
        Ok(Expr::Class { name, extends, body })
    }

    /// Parse an optional `extends <expression>` clause.
    fn class_extends(&mut self) -> JsResult<Option<Box<Expr>>> {
        if self.at_ident("extends") {
            self.advance();
            Ok(Some(Box::new(self.unary()?)))
        } else {
            Ok(None)
        }
    }

    fn class_body(&mut self) -> JsResult<Vec<ClassElement>> {
        let mut elements = Vec::new();
        while !self.at(&TokenKind::RightBrace) && !self.at(&TokenKind::Eof) {
            if self.eat(&TokenKind::Semicolon) {
                continue;
            }
            elements.push(self.class_element()?);
        }
        self.expect(&TokenKind::RightBrace)?;
        Ok(elements)
    }

    fn class_element(&mut self) -> JsResult<ClassElement> {
        let mut is_static = false;
        if self.at_ident("static") && !self.next_is(&TokenKind::LeftParen) {
            self.advance();
            is_static = true;
        }
        let mut is_async = false;
        if self.at_ident("async")
            && !self.next_is(&TokenKind::LeftParen)
            && !self.next_is(&TokenKind::Assign)
        {
            self.advance();
            is_async = true;
        }
        let is_generator = self.eat(&TokenKind::Star);
        let is_getter = self.at_ident("get") && self.next_is_property_name();
        let is_setter = !is_getter && self.at_ident("set") && self.next_is_property_name();
        if is_getter || is_setter {
            self.advance();
        }
        let (name, is_private) = self.class_property_name()?;

        if is_getter {
            self.expect(&TokenKind::LeftParen)?;
            self.expect(&TokenKind::RightParen)?;
            self.expect(&TokenKind::LeftBrace)?;
            let body = self.block()?;
            return Ok(ClassElement::Getter {
                name,
                body,
                is_static,
            });
        }
        if is_setter {
            self.expect(&TokenKind::LeftParen)?;
            let param = self.param_pattern()?;
            self.expect(&TokenKind::RightParen)?;
            self.expect(&TokenKind::LeftBrace)?;
            let body = self.block()?;
            return Ok(ClassElement::Setter {
                name,
                param,
                body,
                is_static,
            });
        }
        if self.eat(&TokenKind::LeftParen) {
            let params = self.params()?;
            self.expect(&TokenKind::LeftBrace)?;
            let body = self.block()?;
            if name == "constructor" && !is_static && !is_generator && !is_async && !is_private {
                return Ok(ClassElement::Constructor { params, body });
            }
            return Ok(ClassElement::Method {
                name,
                params,
                body,
                is_static,
                is_generator,
                is_async,
            });
        }
        // Field definition: `name` or `name = init`.
        let init = if self.eat(&TokenKind::Assign) {
            Some(Box::new(self.assignment()?))
        } else {
            None
        };
        self.optional_semicolon();
        Ok(ClassElement::Field {
            name,
            init,
            is_static,
            is_private,
        })
    }

    /// Parse a class member name: `#private`, a plain identifier/string/number,
    /// or a computed `[expr]` key (stringified).
    fn class_property_name(&mut self) -> JsResult<(String, bool)> {
        if self.at(&TokenKind::LeftBracket) {
            self.advance();
            let key_expr = self.expression()?;
            self.expect(&TokenKind::RightBracket)?;
            let name = format!("computed:{}", Self::key_expr_to_string(&key_expr));
            return Ok((name, false));
        }
        let token = self.advance().clone();
        match token.kind {
            TokenKind::PrivateName(s) => Ok((s, true)),
            TokenKind::Identifier(s) | TokenKind::String(s) => Ok((s, false)),
            TokenKind::Number(n) => Ok((n.to_string(), false)),
            _ => Err(JsError::parse("expected class member name", token.span)),
        }
    }

    /// Whether the current token is an `Identifier` with the given text.
    fn at_ident(&self, name: &str) -> bool {
        matches!(&self.current().kind, TokenKind::Identifier(s) if s == name)
    }

    /// Whether the token following the current one matches `kind`.
    fn next_is(&self, kind: &TokenKind) -> bool {
        self.tokens
            .get(self.pos + 1)
            .is_some_and(|t| std::mem::discriminant(&t.kind) == std::mem::discriminant(kind))
    }

    /// Whether the token following the current one can start a class member
    /// name (used to disambiguate `get`/`set` accessors from methods).
    fn next_is_property_name(&self) -> bool {
        self.tokens.get(self.pos + 1).is_some_and(|t| {
            matches!(
                t.kind,
                TokenKind::Identifier(_)
                    | TokenKind::PrivateName(_)
                    | TokenKind::String(_)
                    | TokenKind::Number(_)
                    | TokenKind::LeftBracket
            )
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
        // Restricted production: `throw [no LineTerminator here] Expression`.
        // A line terminator after `throw` forces ASI, leaving `throw;` with a
        // missing expression, which is a SyntaxError.
        if self.current().span.line != self.previous_span().line {
            return Err(self.error("no line terminator allowed after `throw`"));
        }
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
            if self.eat(&TokenKind::LeftParen) {
                catch_param = Some(self.identifier()?);
                self.expect(&TokenKind::RightParen)?;
            }
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
        // ASI cannot insert a semicolon before `else` on the same line.
        // `if (x) a else b` is a SyntaxError, while `if (x) a; else b` and
        // `if (x) a\nelse b` are fine. A then-branch already terminated by a
        // `;` or a closing `}` needs no inserted semicolon.
        if self.at(&TokenKind::Else) {
            let prev = self.previous_span();
            let terminated = matches!(
                self.tokens.get(self.pos.wrapping_sub(1)).map(|t| &t.kind),
                Some(TokenKind::Semicolon) | Some(TokenKind::RightBrace)
            );
            if !terminated && self.current().span.line == prev.line {
                return Err(self.error("unexpected `else` on the same line as if-body"));
            }
        }
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
            body: self.iteration_body_as_block()?,
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
            if self.eat(&TokenKind::In) {
                let right = self.expression()?;
                self.expect(&TokenKind::RightParen)?;
                return Ok(Stmt::ForIn {
                    left: Box::new(expr),
                    right,
                    body: self.iteration_body_as_block()?,
                });
            }
            self.expect(&TokenKind::Semicolon)?;
            Some(Box::new(Stmt::Expr(expr)))
        };
        if let Some(init) = &init {
            if let Stmt::VarDecl { name, .. } = init.as_ref() {
                if self.eat(&TokenKind::In) {
                    let Pattern::Identifier(ident) = name else {
                        return Err(self.error("for-in binding must be an identifier"));
                    };
                    let right = self.expression()?;
                    self.expect(&TokenKind::RightParen)?;
                    return Ok(Stmt::ForIn {
                        left: Box::new(Expr::Identifier(ident.clone())),
                        right,
                        body: self.iteration_body_as_block()?,
                    });
                }
            }
            if let Stmt::VarDecls { declarations, .. } = init.as_ref() {
                if declarations.len() == 1 && self.eat(&TokenKind::In) {
                    let Pattern::Identifier(ident) = &declarations[0].0 else {
                        return Err(self.error("for-in binding must be an identifier"));
                    };
                    let right = self.expression()?;
                    self.expect(&TokenKind::RightParen)?;
                    return Ok(Stmt::ForIn {
                        left: Box::new(Expr::Identifier(ident.clone())),
                        right,
                        body: self.iteration_body_as_block()?,
                    });
                }
            }
        }
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
            body: self.iteration_body_as_block()?,
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
            return self.block();
        }
        // An `if`/`else` clause may hold a function declaration in sloppy
        // mode (Annex B.3.3), but not in strict mode.
        self.reject_function_declaration_in_statement_position(true)?;
        Ok(vec![self.statement()?])
    }

    /// Parse the body of an iteration statement (`while`, `for`, `for-in`).
    /// Unlike `if` clauses, these never admit a bare function declaration,
    /// regardless of strict mode.
    fn iteration_body_as_block(&mut self) -> JsResult<Vec<Stmt>> {
        if self.eat(&TokenKind::LeftBrace) {
            return self.block();
        }
        self.reject_function_declaration_in_statement_position(false)?;
        Ok(vec![self.statement()?])
    }

    /// Reject a function declaration used directly where the grammar only
    /// permits a `Statement`. When `strict_only` is set, sloppy-mode programs
    /// are left untouched so Annex B extensions keep working.
    fn reject_function_declaration_in_statement_position(
        &self,
        strict_only: bool,
    ) -> JsResult<()> {
        if strict_only && !self.strict {
            return Ok(());
        }
        if self.at(&TokenKind::Function) || self.at_async_function_decl() {
            return Err(self.error(
                "function declaration not allowed in statement position in strict mode",
            ));
        }
        Ok(())
    }

    /// Whether the cursor sits on `async function`, i.e. an async function
    /// declaration (as opposed to an `async (...) => ...` expression).
    fn at_async_function_decl(&self) -> bool {
        self.at(&TokenKind::Async)
            && self
                .tokens
                .get(self.pos + 1)
                .is_some_and(|t| matches!(t.kind, TokenKind::Function))
    }

    fn block(&mut self) -> JsResult<Vec<Stmt>> {
        let mut stmts = Vec::new();
        while !self.at(&TokenKind::RightBrace) && !self.at(&TokenKind::Eof) {
            stmts.push(self.statement()?);
        }
        self.expect(&TokenKind::RightBrace)?;
        Ok(stmts)
    }

    fn params(&mut self) -> JsResult<Vec<Pattern>> {
        let mut params = Vec::new();
        if self.eat(&TokenKind::RightParen) {
            return Ok(params);
        }
        loop {
            let is_rest = self.at(&TokenKind::DotDotDot);
            params.push(self.param_pattern()?);
            if is_rest {
                // A rest parameter must be the final parameter.
                self.expect(&TokenKind::RightParen)?;
                return Ok(params);
            }
            if self.eat(&TokenKind::RightParen) {
                break;
            }
            self.expect(&TokenKind::Comma)?;
        }
        Ok(params)
    }

    /// Parse a single function parameter binding, including `...rest` and
    /// `pattern = default` forms.
    fn param_pattern(&mut self) -> JsResult<Pattern> {
        if self.eat(&TokenKind::DotDotDot) {
            let inner = self.pattern()?;
            return Ok(Pattern::Rest(Box::new(inner)));
        }
        let mut pattern = self.pattern()?;
        if self.eat(&TokenKind::Assign) {
            let default = self.assignment()?;
            pattern = Pattern::Default(Box::new(pattern), default);
        }
        Ok(pattern)
    }

    /// Parse a binding target: an identifier, `[...]` array pattern, or `{...}`
    /// object pattern.
    fn pattern(&mut self) -> JsResult<Pattern> {
        if self.eat(&TokenKind::LeftBracket) {
            self.array_pattern()
        } else if self.eat(&TokenKind::LeftBrace) {
            self.object_pattern()
        } else {
            Ok(Pattern::Identifier(self.identifier()?))
        }
    }

    fn array_pattern(&mut self) -> JsResult<Pattern> {
        let mut elements = Vec::new();
        loop {
            if self.eat(&TokenKind::RightBracket) {
                break;
            }
            if self.eat(&TokenKind::DotDotDot) {
                let inner = self.pattern()?;
                elements.push(Pattern::Rest(Box::new(inner)));
                // Rest must be the final element.
                self.eat(&TokenKind::Comma);
                self.expect(&TokenKind::RightBracket)?;
                break;
            }
            // Elision: a `,` at the start of an element means a hole.
            // Represent it as an Identifier placeholder with a default of
            // undefined (the interpreter binds undefined for holes).
            if self.at(&TokenKind::Comma) {
                elements.push(Pattern::Identifier("".to_string()));
            } else {
                let mut element = self.pattern()?;
                if self.eat(&TokenKind::Assign) {
                    let default = self.assignment()?;
                    element = Pattern::Default(Box::new(element), default);
                }
                elements.push(element);
            }
            if self.eat(&TokenKind::RightBracket) {
                break;
            }
            self.expect(&TokenKind::Comma)?;
            if self.eat(&TokenKind::RightBracket) {
                break;
            }
        }
        Ok(Pattern::ArrayPattern(elements))
    }

    fn object_pattern(&mut self) -> JsResult<Pattern> {
        let mut entries = Vec::new();
        loop {
            if self.eat(&TokenKind::RightBrace) {
                break;
            }
            if self.eat(&TokenKind::DotDotDot) {
                // Object rest: `{ a, ...rest }` is stored as a `"..."` entry.
                let inner = self.pattern()?;
                entries.push(ObjectPatternEntry {
                    key: "...".to_string(),
                    value: Pattern::Rest(Box::new(inner)),
                });
                self.eat(&TokenKind::Comma);
                self.expect(&TokenKind::RightBrace)?;
                break;
            }
            // Computed property: `[expr]: pattern`
            if self.at(&TokenKind::LeftBracket) {
                self.advance();
                let key_expr = self.expression()?;
                self.expect(&TokenKind::RightBracket)?;
                self.expect(&TokenKind::Colon)?;
                let mut pattern = self.pattern()?;
                if self.eat(&TokenKind::Assign) {
                    let default = self.assignment()?;
                    pattern = Pattern::Default(Box::new(pattern), default);
                }
                let key = format!("computed:{}", Self::key_expr_to_string(&key_expr));
                entries.push(ObjectPatternEntry { key, value: pattern });
                if self.eat(&TokenKind::RightBrace) {
                    break;
                }
                self.expect(&TokenKind::Comma)?;
                if self.eat(&TokenKind::RightBrace) {
                    break;
                }
                continue;
            }
            let key = match self.advance().clone().kind {
                TokenKind::Identifier(s) | TokenKind::String(s) => s,
                TokenKind::Number(n) => n.to_string(),
                _ => return Err(self.error("expected object pattern property name")),
            };
            let value = if self.eat(&TokenKind::Colon) {
                let mut pattern = self.pattern()?;
                if self.eat(&TokenKind::Assign) {
                    let default = self.assignment()?;
                    pattern = Pattern::Default(Box::new(pattern), default);
                }
                pattern
            } else {
                let mut pattern = Pattern::Identifier(key.clone());
                if self.eat(&TokenKind::Assign) {
                    let default = self.assignment()?;
                    pattern = Pattern::Default(Box::new(pattern), default);
                }
                pattern
            };
            entries.push(ObjectPatternEntry { key, value });
            if self.eat(&TokenKind::RightBrace) {
                break;
            }
            self.expect(&TokenKind::Comma)?;
            if self.eat(&TokenKind::RightBrace) {
                break;
            }
        }
        Ok(Pattern::ObjectPattern(entries))
    }

    fn expression(&mut self) -> JsResult<Expr> {
        self.assignment()
    }

    fn assignment(&mut self) -> JsResult<Expr> {
        // Destructuring assignment: `[a, b] = rhs` or `{ a, b } = rhs`.
        // Try to read the LHS as a binding pattern and only commit when a
        // plain `=` (never `=>`) follows. Otherwise rewind and let the normal
        // expression path handle it as an array/object literal.
        if self.at(&TokenKind::LeftBracket) || self.at(&TokenKind::LeftBrace) {
            let saved = self.pos;
            if let Ok(pattern) = self.pattern() {
                if self.eat(&TokenKind::Assign) {
                    let value = self.assignment()?;
                    return Ok(Expr::DestructuringAssign {
                        pattern: Box::new(pattern),
                        value: Box::new(value),
                    });
                }
            }
            self.pos = saved;
        }
        let expr = self.conditional()?;
        if self.eat(&TokenKind::Arrow) {
            let params = match expr {
                Expr::Identifier(name) => vec![Pattern::Identifier(name)],
                _ => return Err(self.error("arrow function params must be identifier")),
            };
            return self.arrow_body(params);
        }
        if self.eat(&TokenKind::Assign) {
            if Self::is_assignable(&expr) {
                return Ok(Expr::Assign {
                    target: Box::new(expr),
                    value: Box::new(self.assignment()?),
                });
            }
            // A literal-shaped LHS (`[1, 2] = ...`, `({a: b} = ...)`) is not a
            // valid assignment target, but it can still be reinterpreted as a
            // destructuring pattern. Non-bindable elements become holes.
            if let Some(pattern) = Self::expr_to_assign_pattern(&expr) {
                return Ok(Expr::DestructuringAssign {
                    pattern: Box::new(pattern),
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
            Self::bitwise_or,
            &[
                (TokenKind::Equal, BinaryOp::Equal),
                (TokenKind::NotEqual, BinaryOp::NotEqual),
                (TokenKind::StrictEqual, BinaryOp::StrictEqual),
                (TokenKind::StrictNotEqual, BinaryOp::StrictNotEqual),
            ],
        )
    }

    fn bitwise_or(&mut self) -> JsResult<Expr> {
        self.binary(Self::bitwise_xor, &[(TokenKind::Pipe, BinaryOp::BitwiseOr)])
    }

    fn bitwise_xor(&mut self) -> JsResult<Expr> {
        self.binary(Self::bitwise_and, &[(TokenKind::Caret, BinaryOp::BitwiseXor)])
    }

    fn bitwise_and(&mut self) -> JsResult<Expr> {
        self.binary(Self::comparison, &[(TokenKind::Ampersand, BinaryOp::BitwiseAnd)])
    }

    fn comparison(&mut self) -> JsResult<Expr> {
        self.binary(
            Self::shift,
            &[
                (TokenKind::Less, BinaryOp::Less),
                (TokenKind::LessEqual, BinaryOp::LessEqual),
                (TokenKind::Greater, BinaryOp::Greater),
                (TokenKind::GreaterEqual, BinaryOp::GreaterEqual),
                (TokenKind::In, BinaryOp::In),
                (TokenKind::Instanceof, BinaryOp::Instanceof),
            ],
        )
    }

    fn shift(&mut self) -> JsResult<Expr> {
        self.binary(
            Self::term,
            &[
                (TokenKind::LeftShift, BinaryOp::LeftShift),
                (TokenKind::RightShift, BinaryOp::RightShift),
                (TokenKind::UnsignedRightShift, BinaryOp::UnsignedRightShift),
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
        } else if self.eat(&TokenKind::Delete) {
            Ok(Expr::Unary {
                op: UnaryOp::Delete,
                expr: Box::new(self.unary()?),
            })
        } else if self.eat(&TokenKind::Void) {
            Ok(Expr::Unary {
                op: UnaryOp::Void,
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
            let new_expr = Expr::New {
                callee: Box::new(callee),
                args,
            };
            // Allow `new C().m()` / `new C().x` member and call chains.
            self.postfix(new_expr)
        } else if self.eat(&TokenKind::Await) {
            Ok(Expr::Await(Box::new(self.unary()?)))
        } else if self.eat(&TokenKind::Yield) {
            // `yield`, `yield expr`, or `yield* expr` (delegation is treated
            // as a plain yield for now). A line terminator after `yield`
            // forces it to be a bare `yield` via ASI.
            if self.eat(&TokenKind::Star) {
                // Delegating yield: consume the iterable expression; the
                // interpreter currently treats it as a normal yield.
            }
            let argument = if self.at(&TokenKind::Semicolon)
                || self.at(&TokenKind::RightBrace)
                || self.at(&TokenKind::RightParen)
                || self.at(&TokenKind::RightBracket)
                || self.at(&TokenKind::Comma)
                || self.at(&TokenKind::Eof)
                || self.current().span.line != self.previous_span().line
            {
                Expr::Undefined
            } else {
                self.assignment()?
            };
            Ok(Expr::Yield(Box::new(argument)))
        } else if self.eat(&TokenKind::Tilde) {
            Ok(Expr::Unary {
                op: UnaryOp::BitwiseNot,
                expr: Box::new(self.unary()?),
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
                let property = self.dot_property()?;
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
        let expr = self.primary()?;
        self.postfix(expr)
    }

    /// Apply any trailing member accesses, calls, or postfix `++`/`--` to an
    /// already-parsed primary expression.
    fn postfix(&mut self, mut expr: Expr) -> JsResult<Expr> {
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
                let property = self.dot_property()?;
                expr = Expr::Member {
                    object: Box::new(expr),
                    property,
                };
            } else if self.at(&TokenKind::PlusPlus)
                && self.current().span.line == self.previous_span().line
            {
                // Restricted production: the postfix operator must be on the
                // same line as its operand. Across a line break ASI fires and
                // the `++` starts a new statement instead.
                self.pos += 1;
                if Self::is_assignable(&expr) {
                    expr = Expr::Update {
                        target: Box::new(expr),
                        delta: 1.0,
                        prefix: false,
                    };
                } else {
                    return Err(self.error("increment target must be assignable"));
                }
            } else if self.at(&TokenKind::MinusMinus)
                && self.current().span.line == self.previous_span().line
            {
                self.pos += 1;
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
            // Trailing comma: `f(1, 2,)`
            if self.at(&TokenKind::RightParen) {
                break;
            }
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
            TokenKind::BigInt(s) => Ok(Expr::BigInt(s)),
            TokenKind::String(s) => {
                if s.contains("${") {
                    self.parse_template_string(s)
                } else {
                    Ok(Expr::String(s))
                }
            }
            TokenKind::True => Ok(Expr::Bool(true)),
            TokenKind::False => Ok(Expr::Bool(false)),
            TokenKind::Null => Ok(Expr::Null),
            TokenKind::Undefined => Ok(Expr::Undefined),
            TokenKind::This => Ok(Expr::This),
            TokenKind::Super => Ok(Expr::Super),
            TokenKind::Class => self.class_expr(),
            TokenKind::Identifier(s) => Ok(Expr::Identifier(s)),
            TokenKind::Function => self.function_expr(),
            TokenKind::Async => self.async_function_expr(),
            TokenKind::RegExp(pattern, flags) => Ok(Expr::RegExp {
                pattern: pattern.clone(),
                flags: flags.clone(),
            }),
            TokenKind::LeftParen => {
                let saved = self.pos;
                if let Ok(af) = self.try_arrow_function() {
                    return Ok(af);
                }
                self.pos = saved;
                let e = self.expression()?;
                self.expect(&TokenKind::RightParen)?;
                Ok(e)
            }
            TokenKind::LeftBracket => self.array_literal(),
            TokenKind::LeftBrace => self.object_literal(),
            _ => Err(JsError::parse("expected expression", token.span)),
        }
    }

    fn try_arrow_function(&mut self) -> JsResult<Expr> {
        if self.eat(&TokenKind::RightParen) {
            self.expect(&TokenKind::Arrow)?;
            return self.arrow_body(Vec::new());
        }
        let mut params = Vec::new();
        loop {
            params.push(self.param_pattern()?);
            if self.eat(&TokenKind::RightParen) {
                self.expect(&TokenKind::Arrow)?;
                return self.arrow_body(params);
            }
            self.expect(&TokenKind::Comma)?;
        }
    }

    fn arrow_body(&mut self, params: Vec<Pattern>) -> JsResult<Expr> {
        let body = if self.eat(&TokenKind::LeftBrace) {
            self.block()?
        } else {
            let expr = self.expression()?;
            vec![Stmt::Return(Some(expr))]
        };
        Ok(Expr::ArrowFunction { params, body })
    }

    fn function_expr(&mut self) -> JsResult<Expr> {
        // Optional generator marker: `function* [name](...) { ... }`.
        let generator = self.eat(&TokenKind::Star);
        let name = self.optional_identifier()?;
        self.expect(&TokenKind::LeftParen)?;
        let params = self.params()?;
        self.expect(&TokenKind::LeftBrace)?;
        Ok(Expr::Function {
            name,
            params,
            body: self.block()?,
            generator,
        })
    }

    fn async_function_expr(&mut self) -> JsResult<Expr> {
        // `async function [*] [name] (...) { ... }` or `async (params) => body`
        if self.eat(&TokenKind::Function) {
            let generator = self.eat(&TokenKind::Star);
            let name = self.optional_identifier()?;
            self.expect(&TokenKind::LeftParen)?;
            let params = self.params()?;
            self.expect(&TokenKind::LeftBrace)?;
            return Ok(Expr::AsyncFunction {
                name,
                params,
                body: self.block()?,
                generator,
            });
        }
        // `async (params) => body`
        self.expect(&TokenKind::LeftParen)?;
        let params = self.params()?;
        self.expect(&TokenKind::Arrow)?;
        let body = if self.eat(&TokenKind::LeftBrace) {
            self.block()?
        } else {
            let expr = self.expression()?;
            vec![Stmt::Return(Some(expr))]
        };
        Ok(Expr::AsyncFunction {
            name: None,
            params,
            body,
            generator: false,
        })
    }

    fn parse_template_string(&mut self, s: String) -> JsResult<Expr> {
        // The lexer emitted the whole template as a single String token with
        // `${...}` as literal text. Split into text parts and expression
        // parts, re-lexing each expression fragment.
        let mut parts = Vec::new();
        let mut rest = s;
        while let Some(idx) = rest.find("${") {
            if idx > 0 {
                parts.push(Expr::String(rest[..idx].to_string()));
            }
            let after_dollar = &rest[idx + 2..];
            let mut depth = 1;
            let mut end = None;
            for (i, ch) in after_dollar.char_indices() {
                match ch {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            end = Some(i);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let Some(end) = end else {
                return Err(JsError::parse(
                    "unterminated template expression",
                    Span::new(0, 0, 0, 0),
                ));
            };
            let expr_text = after_dollar[..end].to_string();
            let expr_tokens = crate::lexer::lex(&expr_text)?;
            let expr = crate::parser::parse(expr_tokens)?.statements;
            // The expression should be a single expression statement.
            let expr = match expr.first() {
                Some(Stmt::Expr(e)) => (*e).clone(),
                _ => {
                    return Err(JsError::parse(
                        "template expression must be a single expression",
                        Span::new(0, 0, 0, 0),
                    ))
                }
            };
            parts.push(expr);
            rest = after_dollar[end + 1..].to_string();
        }
        if !rest.is_empty() {
            parts.push(Expr::String(rest));
        }
        Ok(Expr::TemplateLiteral { parts })
    }

    fn array_literal(&mut self) -> JsResult<Expr> {
        let mut items: Vec<Option<Expr>> = Vec::new();
        if self.eat(&TokenKind::RightBracket) {
            return Ok(Expr::Array(items));
        }
        loop {
            // Elision: `,` or `]` after a comma (or at start) means a hole.
            if self.at(&TokenKind::Comma) || self.at(&TokenKind::RightBracket) {
                items.push(None);
            } else {
                items.push(Some(self.expression()?));
            }
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
        let mut props: Vec<(Option<String>, Expr)> = Vec::new();
        if self.eat(&TokenKind::RightBrace) {
            return Ok(Expr::Object(props));
        }
        loop {
            // Spread: `...expr`
            if self.eat(&TokenKind::DotDotDot) {
                let value = self.expression()?;
                props.push((None, value));
                if self.eat(&TokenKind::RightBrace) {
                    break;
                }
                self.expect(&TokenKind::Comma)?;
                if self.eat(&TokenKind::RightBrace) {
                    break;
                }
                continue;
            }
            // Accessor: `get name() {}` or `set name(v) {}`.
            // `get`/`set` are only accessors when followed by a property name
            // (not `(` for a method named `get`, not `=` for a field).
            if (self.at_ident("get") || self.at_ident("set"))
                && self.next_is_property_name()
            {
                let is_getter = self.at_ident("get");
                self.advance(); // consume `get`/`set`
                // Property name (identifier, string, number, computed, or private).
                let key = if self.at(&TokenKind::LeftBracket) {
                    self.advance();
                    let key_expr = self.expression()?;
                    self.expect(&TokenKind::RightBracket)?;
                    Some(format!("computed:{}", Self::key_expr_to_string(&key_expr)))
                } else {
                    match self.advance().clone().kind {
                        TokenKind::Identifier(s) | TokenKind::String(s) => Some(s),
                        TokenKind::Number(n) => Some(n.to_string()),
                        TokenKind::PrivateName(s) => Some(s),
                        _ => return Err(self.error("expected object property name")),
                    }
                };
                self.expect(&TokenKind::LeftParen)?;
                let params = if is_getter {
                    self.expect(&TokenKind::RightParen)?;
                    vec![]
                } else {
                    let param = self.param_pattern()?;
                    self.expect(&TokenKind::RightParen)?;
                    vec![param]
                };
                self.expect(&TokenKind::LeftBrace)?;
                let body = self.block()?;
                let k = key.clone().unwrap_or_default();
                let value = Expr::Function { name: Some(k), params, body, generator: false };
                // Mark as accessor so the interpreter can install it as a getter/setter.
                props.push((Some(format!("__accessor__{}", key.unwrap_or_default())), value));
                if self.eat(&TokenKind::RightBrace) {
                    break;
                }
                self.expect(&TokenKind::Comma)?;
                if self.eat(&TokenKind::RightBrace) {
                    break;
                }
                continue;
            }
            // Computed property: `[expr]: value`
            if self.at(&TokenKind::LeftBracket) {
                self.advance(); // consume `[`
                let key_expr = self.expression()?;
                self.expect(&TokenKind::RightBracket)?;
                self.expect(&TokenKind::Colon)?;
                let value = self.expression()?;
                // Store the computed key as a special marker; we use the
                // stringified form of the key expression for now.
                let key = format!("computed:{}", Self::key_expr_to_string(&key_expr));
                props.push((Some(key), value));
            } else {
                // Generator / async-generator method prefix:
                // `*name(...) {}` or `async *name(...) {}`.
                let mut is_generator = false;
                let mut is_async = false;
                if self.at(&TokenKind::Star) {
                    self.advance();
                    is_generator = true;
                } else if self.at(&TokenKind::Async)
                    && self
                        .tokens
                        .get(self.pos + 1)
                        .is_some_and(|t| matches!(t.kind, TokenKind::Star))
                {
                    self.advance();
                    self.advance();
                    is_generator = true;
                    is_async = true;
                }
                let key = match self.advance().clone().kind {
                    TokenKind::Identifier(s) | TokenKind::String(s) => Some(s),
                    TokenKind::Number(n) => Some(n.to_string()),
                    _ => return Err(self.error("expected object property name")),
                };
                if self.eat(&TokenKind::Colon) {
                    let value = self.expression()?;
                    props.push((key, value));
                } else if self.at(&TokenKind::LeftParen) {
                    // Method definition: `key(params) { body }`
                    self.advance(); // consume `(`
                    let params = self.params()?;
                    self.expect(&TokenKind::LeftBrace)?;
                    let body = self.block()?;
                    let k = key.clone().unwrap_or_default();
                    let value = if is_async {
                        Expr::AsyncFunction {
                            name: Some(k),
                            params,
                            body,
                            generator: true,
                        }
                    } else {
                        Expr::Function {
                            name: Some(k),
                            params,
                            body,
                            generator: is_generator,
                        }
                    };
                    props.push((key, value));
                } else {
                    let k = key.clone().unwrap_or_default();
                    let value = Expr::Identifier(k);
                    props.push((key, value));
                }
            }
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

    /// Reinterpret an already-parsed array/object literal as a destructuring
    /// assignment pattern. Returns `None` when the expression cannot be a
    /// destructuring target at all (e.g. a call expression). Elements that are
    /// not valid binding targets (numbers, calls, ...) are treated as holes so
    /// permissive forms such as `[1, 2] = [3, 4]` still parse.
    fn expr_to_assign_pattern(expr: &Expr) -> Option<Pattern> {
        match expr {
            Expr::Identifier(name) => Some(Pattern::Identifier(name.clone())),
            // `a = default` inside a pattern.
            Expr::Assign { target, value } => {
                let inner = Self::expr_to_assign_pattern(target)?;
                Some(Pattern::Default(Box::new(inner), (**value).clone()))
            }
            Expr::Array(items) => {
                let mut patterns = Vec::with_capacity(items.len());
                for item in items {
                    let pattern = match item {
                        None => Pattern::Identifier(String::new()),
                        Some(e) => Self::expr_to_assign_pattern(e)
                            .unwrap_or_else(|| Pattern::Identifier(String::new())),
                    };
                    patterns.push(pattern);
                }
                Some(Pattern::ArrayPattern(patterns))
            }
            Expr::Object(props) => {
                let mut entries = Vec::with_capacity(props.len());
                for (key, value) in props {
                    match key {
                        // Spread element `...rest`.
                        None => {
                            let inner = Self::expr_to_assign_pattern(value)?;
                            entries.push(ObjectPatternEntry {
                                key: "...".to_string(),
                                value: Pattern::Rest(Box::new(inner)),
                            });
                        }
                        Some(k) => {
                            let pattern = Self::expr_to_assign_pattern(value)
                                .unwrap_or_else(|| Pattern::Identifier(String::new()));
                            entries.push(ObjectPatternEntry {
                                key: k.clone(),
                                value: pattern,
                            });
                        }
                    }
                }
                Some(Pattern::ObjectPattern(entries))
            }
            _ => None,
        }
    }

    /// Short string form of a computed-key expression, used as a property
    /// name placeholder. Only handles the common cases.
    fn key_expr_to_string(expr: &Expr) -> String {
        match expr {
            Expr::Identifier(s) => s.clone(),
            Expr::String(s) => s.clone(),
            Expr::Number(n) => n.to_string(),
            Expr::BigInt(s) => s.clone(),
            other => format!("{:?}", other),
        }
    }

    fn identifier(&mut self) -> JsResult<String> {
        self.identifier_token().map(|(name, _)| name)
    }

    /// Parse the property name after a `.`, allowing private names (`.#x`).
    fn dot_property(&mut self) -> JsResult<String> {
        let token = self.advance().clone();
        match token.kind {
            TokenKind::Identifier(s) | TokenKind::PrivateName(s) => Ok(s),
            _ => Err(JsError::parse("expected property name", token.span)),
        }
    }
    /// Capture an optional function name (Identifier) that follows `function`.
    fn optional_identifier(&mut self) -> JsResult<Option<String>> {
        if matches!(self.peek().kind, TokenKind::Identifier(_)) {
            Ok(Some(self.identifier()?))
        } else {
            Ok(None)
        }
    }
    fn identifier_token(&mut self) -> JsResult<(String, Span)> {
        let token = self.advance().clone();
        if let TokenKind::Identifier(name) = token.kind {
            Ok((name, token.span))
        } else {
            Err(JsError::parse("expected identifier", token.span))
        }
    }

    fn optional_semicolon(&mut self) {
        self.eat(&TokenKind::Semicolon);
    }

    /// Span of the most recently consumed token (the token just before the
    /// cursor). Used by ASI decisions that compare line numbers.
    fn previous_span(&self) -> Span {
        if self.pos == 0 {
            self.current().span
        } else {
            self.tokens[self.pos - 1].span
        }
    }

    fn at_statement_end_after(&self, previous_span: Span) -> bool {
        self.at(&TokenKind::Semicolon)
            || self.at(&TokenKind::RightBrace)
            || self.at(&TokenKind::Eof)
            || self.current().span.line > previous_span.line
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

    fn peek(&self) -> &Token {
        self.current()
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

    #[test]
    fn parses_generator_forms() {
        assert!(parse(lex("function* g(){ yield 1; }").unwrap()).is_ok());
        assert!(parse(lex("var f = function* g(){ yield 1; };").unwrap()).is_ok());
        assert!(parse(lex("var o = { *m(){ yield 1; } };").unwrap()).is_ok());
        assert!(parse(lex("async function* g(){ yield 1; }").unwrap()).is_ok());
        assert!(parse(lex("async function* g(){ yield 1; } ag();").unwrap()).is_ok());
        // Bare `yield` and `yield*`.
        assert!(parse(lex("function* g(){ yield; yield* xs; }").unwrap()).is_ok());
    }

    #[test]
    fn parses_uninitialized_let() {
        assert!(parse(lex("let x; x;").unwrap()).is_ok());
    }

    #[test]
    fn rejects_uninitialized_const() {
        let error = parse(lex("const x;").unwrap()).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("const declarations must be initialized")
        );
    }

    #[test]
    fn parses_uninitialized_let_before_number() {
        assert!(parse(lex("let x\n1;").unwrap()).is_ok());
    }
    #[test]
    fn parses_uninitialized_let_before_newline_statement() {
        assert!(parse(lex("let x\nx = 1;").unwrap()).is_ok());
    }

    #[test]
    fn rejects_function_decl_in_strict_if_branch() {
        assert!(parse(lex("\"use strict\"; if (true) function g() {}").unwrap()).is_err());
    }

    #[test]
    fn rejects_function_decl_in_strict_else_branch() {
        assert!(parse(lex("\"use strict\"; if (true) {} else function g() {}").unwrap()).is_err());
    }

    #[test]
    fn rejects_async_function_decl_in_strict_if_branch() {
        assert!(
            parse(lex("\"use strict\"; if (true) async function g() {}").unwrap()).is_err()
        );
    }

    #[test]
    fn allows_function_decl_in_sloppy_if_branch() {
        assert!(parse(lex("if (true) function g() {}").unwrap()).is_ok());
    }

    #[test]
    fn rejects_function_decl_in_while_body() {
        assert!(parse(lex("while (false) function g() {}").unwrap()).is_err());
    }

    #[test]
    fn rejects_function_decl_in_for_body() {
        assert!(parse(lex("for (;false;) function g() {}").unwrap()).is_err());
    }

    #[test]
    fn allows_strict_top_level_and_block_function_decls() {
        assert!(parse(lex("\"use strict\"; function g() {}").unwrap()).is_ok());
        assert!(parse(lex("\"use strict\"; if (true) { function g() {} }").unwrap()).is_ok());
    }

    // --- ASI edge cases (test262 negative phase: parse) ---

    #[test]
    fn rejects_same_line_expression_statements() {
        // `{1 2} 3` — ASI cannot insert a semicolon between `1` and `2`.
        assert!(parse(lex("{1 2} 3").unwrap()).is_err());
        assert!(parse(lex("{ 1 2 } 3").unwrap()).is_err());
        // Valid: statements separated by a line terminator or `;`.
        assert!(parse(lex("{1; 2} 3").unwrap()).is_ok());
        assert!(parse(lex("{1\n2} 3").unwrap()).is_ok());
    }

    #[test]
    fn rejects_else_on_same_line_as_if_body() {
        assert!(parse(lex("if (false) x = 1 else x = -1").unwrap()).is_err());
        // Valid: a terminating `;`, a block, or a line terminator.
        assert!(parse(lex("if (false) x = 1; else x = -1").unwrap()).is_ok());
        assert!(parse(lex("if (false) { x = 1 } else { x = -1 }").unwrap()).is_ok());
        assert!(parse(lex("if (false) x = 1\nelse x = -1").unwrap()).is_ok());
    }

    #[test]
    fn rejects_line_terminator_after_throw() {
        assert!(parse(lex("throw\n1;").unwrap()).is_err());
        // Valid: the expression starts on the same line.
        assert!(parse(lex("throw 1;").unwrap()).is_ok());
    }

    #[test]
    fn rejects_postfix_update_across_line_terminator() {
        assert!(parse(lex("x\n++;").unwrap()).is_err());
        assert!(parse(lex("x\n--;").unwrap()).is_err());
        // Valid: same line.
        assert!(parse(lex("x++;").unwrap()).is_ok());
        assert!(parse(lex("x--;").unwrap()).is_ok());
        // Valid ASI: `x; ++y;` and `x; --y;`.
        assert!(parse(lex("x\n++y").unwrap()).is_ok());
        assert!(parse(lex("x\n--y").unwrap()).is_ok());
    }
}
