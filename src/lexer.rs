use crate::token::Token;

pub struct Lexer {
    input: Vec<char>,
    pos: usize,
    indent_stack: Vec<usize>,
    at_line_start: bool,
    pending_tokens: Vec<Token>,
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        Lexer {
            input: input.chars().collect(),
            pos: 0,
            indent_stack: vec![0],
            at_line_start: true,
            pending_tokens: Vec::new(),
        }
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>, String> {
        let mut tokens = Vec::new();

        loop {
            // Drain any pending indent/dedent tokens first
            while let Some(tok) = self.pending_tokens.pop() {
                tokens.push(tok);
            }

            if self.pos >= self.input.len() {
                // Emit remaining dedents
                while self.indent_stack.len() > 1 {
                    self.indent_stack.pop();
                    tokens.push(Token::Dedent);
                }
                tokens.push(Token::Eof);
                break;
            }

            if self.at_line_start {
                self.handle_indentation(&mut tokens)?;
                self.at_line_start = false;
                continue;
            }

            let ch = self.current_char();

            // Skip spaces/tabs (not at line start)
            if ch == ' ' || ch == '\t' {
                self.pos += 1;
                continue;
            }

            // Comments
            if ch == '#' {
                while self.pos < self.input.len() && self.input[self.pos] != '\n' {
                    self.pos += 1;
                }
                continue;
            }

            // Newline
            if ch == '\n' {
                tokens.push(Token::Newline);
                self.pos += 1;
                self.at_line_start = true;
                continue;
            }

            // Numbers
            if ch.is_ascii_digit() {
                tokens.push(self.read_number()?);
                continue;
            }

            // Strings
            if ch == '"' || ch == '\'' {
                tokens.push(self.read_string()?);
                continue;
            }

            // Identifiers and keywords
            if ch.is_alphabetic() || ch == '_' {
                tokens.push(self.read_identifier());
                continue;
            }

            // Operators and delimiters
            tokens.push(self.read_operator()?);
        }

        Ok(tokens)
    }

    fn current_char(&self) -> char {
        self.input[self.pos]
    }

    fn peek_char(&self) -> Option<char> {
        self.input.get(self.pos + 1).copied()
    }

    fn handle_indentation(&mut self, tokens: &mut Vec<Token>) -> Result<(), String> {
        // Skip blank lines
        if self.pos < self.input.len() && self.input[self.pos] == '\n' {
            self.pos += 1;
            self.at_line_start = true;
            return Ok(());
        }

        // Count leading spaces
        let mut indent = 0;
        while self.pos < self.input.len() && self.input[self.pos] == ' ' {
            indent += 1;
            self.pos += 1;
        }

        // Skip blank/comment-only lines
        if self.pos >= self.input.len()
            || self.input[self.pos] == '\n'
            || self.input[self.pos] == '#'
        {
            return Ok(());
        }

        let current_indent = *self.indent_stack.last().unwrap();

        if indent > current_indent {
            self.indent_stack.push(indent);
            tokens.push(Token::Indent);
        } else {
            while indent < *self.indent_stack.last().unwrap() {
                self.indent_stack.pop();
                tokens.push(Token::Dedent);
            }
            if indent != *self.indent_stack.last().unwrap() {
                return Err(format!("Indentation error at position {}", self.pos));
            }
        }

        Ok(())
    }

    fn read_number(&mut self) -> Result<Token, String> {
        let start = self.pos;
        let mut is_float = false;

        while self.pos < self.input.len() && self.input[self.pos].is_ascii_digit() {
            self.pos += 1;
        }

        if self.pos < self.input.len() && self.input[self.pos] == '.' {
            is_float = true;
            self.pos += 1;
            while self.pos < self.input.len() && self.input[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        }

        let num_str: String = self.input[start..self.pos].iter().collect();

        if is_float {
            num_str
                .parse::<f64>()
                .map(Token::Float)
                .map_err(|e| format!("Invalid float: {}", e))
        } else {
            num_str
                .parse::<i64>()
                .map(Token::Integer)
                .map_err(|e| format!("Invalid integer: {}", e))
        }
    }

    fn read_string(&mut self) -> Result<Token, String> {
        let quote = self.input[self.pos];
        self.pos += 1; // skip opening quote
        let mut s = String::new();

        while self.pos < self.input.len() && self.input[self.pos] != quote {
            if self.input[self.pos] == '\\' {
                self.pos += 1;
                if self.pos >= self.input.len() {
                    return Err("Unterminated string escape".to_string());
                }
                match self.input[self.pos] {
                    'n' => s.push('\n'),
                    't' => s.push('\t'),
                    '\\' => s.push('\\'),
                    '\'' => s.push('\''),
                    '"' => s.push('"'),
                    _ => {
                        s.push('\\');
                        s.push(self.input[self.pos]);
                    }
                }
            } else {
                s.push(self.input[self.pos]);
            }
            self.pos += 1;
        }

        if self.pos >= self.input.len() {
            return Err("Unterminated string".to_string());
        }

        self.pos += 1; // skip closing quote
        Ok(Token::StringLiteral(s))
    }

    fn read_identifier(&mut self) -> Token {
        let start = self.pos;
        while self.pos < self.input.len()
            && (self.input[self.pos].is_alphanumeric() || self.input[self.pos] == '_')
        {
            self.pos += 1;
        }

        let word: String = self.input[start..self.pos].iter().collect();

        Token::keyword_from_str(&word).unwrap_or(Token::Identifier(word))
    }

    fn read_operator(&mut self) -> Result<Token, String> {
        let ch = self.input[self.pos];
        self.pos += 1;

        match ch {
            '+' => {
                if self.pos < self.input.len() && self.input[self.pos] == '=' {
                    self.pos += 1;
                    Ok(Token::PlusAssign)
                } else {
                    Ok(Token::Plus)
                }
            }
            '-' => {
                if self.pos < self.input.len() && self.input[self.pos] == '>' {
                    self.pos += 1;
                    Ok(Token::Arrow)
                } else if self.pos < self.input.len() && self.input[self.pos] == '=' {
                    self.pos += 1;
                    Ok(Token::MinusAssign)
                } else {
                    Ok(Token::Minus)
                }
            }
            '*' => {
                if self.pos < self.input.len() && self.input[self.pos] == '*' {
                    self.pos += 1;
                    Ok(Token::DoubleStar)
                } else if self.pos < self.input.len() && self.input[self.pos] == '=' {
                    self.pos += 1;
                    Ok(Token::StarAssign)
                } else {
                    Ok(Token::Star)
                }
            }
            '/' => {
                if self.pos < self.input.len() && self.input[self.pos] == '/' {
                    self.pos += 1;
                    Ok(Token::DoubleSlash)
                } else if self.pos < self.input.len() && self.input[self.pos] == '=' {
                    self.pos += 1;
                    Ok(Token::SlashAssign)
                } else {
                    Ok(Token::Slash)
                }
            }
            '%' => Ok(Token::Percent),
            '=' => {
                if self.pos < self.input.len() && self.input[self.pos] == '=' {
                    self.pos += 1;
                    Ok(Token::Equal)
                } else {
                    Ok(Token::Assign)
                }
            }
            '!' => {
                if self.pos < self.input.len() && self.input[self.pos] == '=' {
                    self.pos += 1;
                    Ok(Token::NotEqual)
                } else {
                    Err(format!("Unexpected character: !", ))
                }
            }
            '<' => {
                if self.pos < self.input.len() && self.input[self.pos] == '=' {
                    self.pos += 1;
                    Ok(Token::LessEqual)
                } else {
                    Ok(Token::Less)
                }
            }
            '>' => {
                if self.pos < self.input.len() && self.input[self.pos] == '=' {
                    self.pos += 1;
                    Ok(Token::GreaterEqual)
                } else {
                    Ok(Token::Greater)
                }
            }
            '(' => Ok(Token::LeftParen),
            ')' => Ok(Token::RightParen),
            '[' => Ok(Token::LeftBracket),
            ']' => Ok(Token::RightBracket),
            '{' => Ok(Token::LeftBrace),
            '}' => Ok(Token::RightBrace),
            ',' => Ok(Token::Comma),
            ':' => Ok(Token::Colon),
            '.' => Ok(Token::Dot),
            _ => Err(format!("Unexpected character: {}", ch)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_assignment() {
        let mut lexer = Lexer::new("x = 42\n");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::Identifier("x".to_string()),
                Token::Assign,
                Token::Integer(42),
                Token::Newline,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn test_function_def() {
        let mut lexer = Lexer::new("def add(a, b):\n    return a + b\n");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::Def,
                Token::Identifier("add".to_string()),
                Token::LeftParen,
                Token::Identifier("a".to_string()),
                Token::Comma,
                Token::Identifier("b".to_string()),
                Token::RightParen,
                Token::Colon,
                Token::Newline,
                Token::Indent,
                Token::Return,
                Token::Identifier("a".to_string()),
                Token::Plus,
                Token::Identifier("b".to_string()),
                Token::Newline,
                Token::Dedent,
                Token::Eof,
            ]
        );
    }
}
