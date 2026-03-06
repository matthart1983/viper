use std::collections::HashMap;

/// A compact identifier, used in place of string comparisons at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Symbol(pub u32);

/// Maps strings → Symbol IDs and back. All identifier lookups at runtime
/// use integer keys instead of string hashing.
pub struct Interner {
    map: HashMap<String, u32>,
    names: Vec<String>,
}

impl Interner {
    pub fn new() -> Self {
        Interner {
            map: HashMap::new(),
            names: Vec::new(),
        }
    }

    pub fn intern(&mut self, name: &str) -> Symbol {
        if let Some(&id) = self.map.get(name) {
            return Symbol(id);
        }
        let id = self.names.len() as u32;
        self.names.push(name.to_string());
        self.map.insert(name.to_string(), id);
        Symbol(id)
    }

    pub fn resolve(&self, sym: Symbol) -> &str {
        &self.names[sym.0 as usize]
    }
}
