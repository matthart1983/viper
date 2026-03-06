#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Literals
    Integer(i64),
    Float(f64),
    StringLiteral(String),
    Boolean(bool),
    Identifier(String),

    // Keywords
    Def,
    Return,
    If,
    Elif,
    Else,
    While,
    For,
    In,
    Break,
    Continue,
    Pass,
    None,
    True,
    False,
    And,
    Or,
    Not,
    Print,
    Class,
    Import,
    From,
    As,

    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    DoubleSlash,
    Percent,
    DoubleStar,
    Assign,
    PlusAssign,
    MinusAssign,
    StarAssign,
    SlashAssign,

    // Comparison
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,

    // Delimiters
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    LeftBrace,
    RightBrace,
    Comma,
    Colon,
    Dot,
    Arrow,

    // Indentation
    Indent,
    Dedent,
    Newline,

    // Special
    Eof,
}

impl Token {
    pub fn keyword_from_str(s: &str) -> Option<Token> {
        match s {
            "def" => Some(Token::Def),
            "return" => Some(Token::Return),
            "if" => Some(Token::If),
            "elif" => Some(Token::Elif),
            "else" => Some(Token::Else),
            "while" => Some(Token::While),
            "for" => Some(Token::For),
            "in" => Some(Token::In),
            "break" => Some(Token::Break),
            "continue" => Some(Token::Continue),
            "pass" => Some(Token::Pass),
            "None" => Some(Token::None),
            "True" => Some(Token::True),
            "False" => Some(Token::False),
            "and" => Some(Token::And),
            "or" => Some(Token::Or),
            "not" => Some(Token::Not),
            "print" => Some(Token::Print),
            "class" => Some(Token::Class),
            "import" => Some(Token::Import),
            "from" => Some(Token::From),
            "as" => Some(Token::As),
            _ => None,
        }
    }
}
