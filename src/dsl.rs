use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Ident(String),
    Number(i64),
    Colon,
    Comma,
    LParen,
    RParen,
    LBrace,
    RBrace,
    Question,
}

pub fn tokenize(input: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();

    while let Some(&c) = chars.peek() {
        match c {
            ' ' | '\n' | '\r' | '\t' => {
                chars.next();
            }
            ':' => {
                chars.next();
                tokens.push(Token::Colon);
            }
            ',' => {
                chars.next();
                tokens.push(Token::Comma);
            }
            '(' => {
                chars.next();
                tokens.push(Token::LParen);
            }
            ')' => {
                chars.next();
                tokens.push(Token::RParen);
            }
            '{' => {
                chars.next();
                tokens.push(Token::LBrace);
            }
            '}' => {
                chars.next();
                tokens.push(Token::RBrace);
            }
            '?' => {
                chars.next();
                tokens.push(Token::Question);
            }
            '0'..='9' => {
                let mut num = String::new();
                while let Some(&d) = chars.peek() {
                    if d.is_ascii_digit() {
                        num.push(d);
                        chars.next();
                    } else {
                        break;
                    }
                }
                tokens.push(Token::Number(num.parse().map_err(|_| "Invalid number")?));
            }
            _ => {
                if c.is_alphanumeric()
                    || c == '_'
                    || c == '^'
                    || c == '$'
                    || c == '['
                    || c == ']'
                    || c == '+'
                    || c == '*'
                    || c == '.'
                {
                    let mut ident = String::new();
                    while let Some(&ch) = chars.peek() {
                        if ch.is_alphanumeric() || "_^$[]+*.-".contains(ch) {
                            ident.push(ch);
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    tokens.push(Token::Ident(ident));
                } else {
                    return Err(format!("Unexpected character: {}", c));
                }
            }
        }
    }

    Ok(tokens)
}

#[derive(Debug)]
pub struct Program {
    pub sections: Vec<Section>,
}

#[derive(Debug)]
pub struct Section {
    pub source: Source,
    pub fields: Vec<FieldAst>,
}

#[derive(Debug, Clone)]
pub enum Source {
    Body,
    Query,
    Params,
}

#[derive(Debug)]
pub struct FieldAst {
    pub name: String,
    pub field_type: FieldTypeAst,
    pub constraints: Vec<ConstraintAst>,
    pub required: bool,
}

#[derive(Debug)]
pub enum FieldTypeAst {
    String,
    Int,
    Bool,
}

#[derive(Debug)]
pub enum ConstraintAst {
    Min(i64),
    Max(i64),
    Length(i64, i64),
    Regex(String),
    Array,
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<Token> {
        let t = self.tokens.get(self.pos).cloned();
        self.pos += 1;
        t
    }

    fn expect(&mut self, expected: &Token) -> Result<(), String> {
        let t = self.next().ok_or("Unexpected EOF")?;
        if &t != expected {
            return Err(format!("Expected {:?}, got {:?}", expected, t));
        }
        Ok(())
    }

    pub fn parse_program(&mut self) -> Result<Program, String> {
        let mut sections = Vec::new();
        while self.peek().is_some() {
            sections.push(self.parse_section()?);
        }
        Ok(Program { sections })
    }

    fn parse_section(&mut self) -> Result<Section, String> {
        let source = match self.next() {
            Some(Token::Ident(s)) if s == "body" => Source::Body,
            Some(Token::Ident(s)) if s == "query" => Source::Query,
            Some(Token::Ident(s)) if s == "params" => Source::Params,
            t => return Err(format!("Invalid source: {:?}", t)),
        };

        self.expect(&Token::LParen)?;

        let mut fields = Vec::new();
        loop {
            fields.push(self.parse_field()?);

            match self.peek() {
                Some(Token::Comma) => {
                    self.next();
                }
                Some(Token::RParen) => {
                    self.next();
                    break;
                }
                _ => return Err("Expected ',' or ')'".into()),
            }
        }

        Ok(Section { source, fields })
    }

    fn parse_field(&mut self) -> Result<FieldAst, String> {
        let name = match self.next() {
            Some(Token::Ident(s)) => s,
            t => return Err(format!("Expected field name, got {:?}", t)),
        };

        self.expect(&Token::Colon)?;

        let field_type = match self.next() {
            Some(Token::Ident(s)) if s == "string" => FieldTypeAst::String,
            Some(Token::Ident(s)) if s == "int" => FieldTypeAst::Int,
            Some(Token::Ident(s)) if s == "bool" => FieldTypeAst::Bool,
            t => return Err(format!("Invalid type {:?}", t)),
        };

        let mut constraints = Vec::new();

        if matches!(self.peek(), Some(Token::LBrace)) {
            self.next();

            loop {
                constraints.push(self.parse_constraint()?);

                match self.peek() {
                    Some(Token::Comma) => {
                        self.next();
                    }
                    Some(Token::RBrace) => {
                        self.next();
                        break;
                    }
                    _ => return Err("Expected ',' or '}'".into()),
                }
            }
        }

        let required = !matches!(self.peek(), Some(Token::Question));
        if !required {
            self.next();
        }

        Ok(FieldAst {
            name,
            field_type,
            constraints,
            required,
        })
    }

    fn parse_constraint(&mut self) -> Result<ConstraintAst, String> {
        match self.next() {
            Some(Token::Ident(s)) if s == "min" => {
                self.expect(&Token::LParen)?;
                let v = self.expect_number()?;
                self.expect(&Token::RParen)?;
                Ok(ConstraintAst::Min(v))
            }
            Some(Token::Ident(s)) if s == "max" => {
                self.expect(&Token::LParen)?;
                let v = self.expect_number()?;
                self.expect(&Token::RParen)?;
                Ok(ConstraintAst::Max(v))
            }
            Some(Token::Ident(s)) if s == "length" => {
                self.expect(&Token::LParen)?;
                let a = self.expect_number()?;
                self.expect(&Token::Comma)?;
                let b = self.expect_number()?;
                self.expect(&Token::RParen)?;
                Ok(ConstraintAst::Length(a, b))
            }
            Some(Token::Ident(s)) if s == "regex" => {
                self.expect(&Token::LParen)?;
                let pattern = match self.next() {
                    Some(Token::Ident(p)) => p,
                    t => return Err(format!("Invalid regex pattern {:?}", t)),
                };
                self.expect(&Token::RParen)?;
                Ok(ConstraintAst::Regex(pattern))
            }
            Some(Token::Ident(s)) if s == "array" => Ok(ConstraintAst::Array),
            t => Err(format!("Invalid constraint {:?}", t)),
        }
    }

    fn expect_number(&mut self) -> Result<i64, String> {
        match self.next() {
            Some(Token::Number(n)) => Ok(n),
            t => Err(format!("Expected number, got {:?}", t)),
        }
    }
}

pub fn parse_rules(input: &str) -> Result<Program, String> {
    let tokens = tokenize(input)?;
    let mut parser = Parser::new(tokens);
    parser.parse_program()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_basic() {
        let dsl = r#"
            body(
                username:string{length(2,10)},
                password:string
            )
        "#;

        let program = parse_rules(dsl).unwrap();
        assert_eq!(program.sections.len(), 1);
        assert_eq!(program.sections[0].fields.len(), 2);
    }

    #[test]
    fn test_optional() {
        let dsl = r#"
            query(
                page:int{min(1)}?
            )
        "#;

        let program = parse_rules(dsl).unwrap();
        let field = &program.sections[0].fields[0];
        assert!(!field.required);
    }
}
