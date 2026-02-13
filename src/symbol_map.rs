use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};
use std::collections::HashMap;

/// Maps original class/ID names to obfuscated replacements.
///
/// When a seed is provided, the mapping is deterministic.
#[derive(Debug)]
pub struct SymbolMap {
    classes: HashMap<String, String>,
    ids: HashMap<String, String>,
    rng: StdRng,
    counter: u32,
}

impl SymbolMap {
    pub fn new(seed: Option<u64>) -> Self {
        let rng = match seed {
            Some(s) => StdRng::seed_from_u64(s),
            None => StdRng::from_rng(&mut rand::rng()),
        };
        Self {
            classes: HashMap::new(),
            ids: HashMap::new(),
            rng,
            counter: 0,
        }
    }

    /// Register a class name for later mapping. Does nothing if already registered.
    pub fn register_class(&mut self, name: &str) {
        if !self.classes.contains_key(name) {
            let obf = self.generate_name();
            self.classes.insert(name.to_owned(), obf);
        }
    }

    /// Register an ID for later mapping. Does nothing if already registered.
    pub fn register_id(&mut self, name: &str) {
        if !self.ids.contains_key(name) {
            let obf = self.generate_name();
            self.ids.insert(name.to_owned(), obf);
        }
    }

    /// Look up the obfuscated name for a class.
    pub fn get_class(&self, name: &str) -> Option<&str> {
        self.classes.get(name).map(|s| s.as_str())
    }

    /// Look up the obfuscated name for an ID.
    pub fn get_id(&self, name: &str) -> Option<&str> {
        self.ids.get(name).map(|s| s.as_str())
    }

    /// Returns all class mappings (original → obfuscated).
    pub fn classes(&self) -> &HashMap<String, String> {
        &self.classes
    }

    /// Returns all ID mappings (original → obfuscated).
    pub fn ids(&self) -> &HashMap<String, String> {
        &self.ids
    }

    /// Resolve compound class names using JS-detected concatenation prefixes.
    ///
    /// For each JS prefix (e.g., `tier-`, `tier-border-`), finds CSS classes
    /// starting with that prefix, registers the suffix as a class, and maps
    /// the compound to prefix + obfuscated(suffix).
    ///
    /// Uses longest-prefix matching so `tier-border-critical` matches
    /// `tier-border-` (not `tier-`).
    pub fn resolve_compounds(&mut self, js_prefixes: &[String]) {
        if js_prefixes.is_empty() {
            return;
        }

        // Sort prefixes by length descending for longest-match-first
        let mut prefixes = js_prefixes.to_vec();
        prefixes.sort_by_key(|b| std::cmp::Reverse(b.len()));

        let class_names: Vec<String> = self.classes.keys().cloned().collect();

        for original in &class_names {
            // Find longest matching prefix
            for prefix in &prefixes {
                if let Some(suffix) = original.strip_prefix(prefix.as_str()) {
                    if !suffix.is_empty() && !suffix.contains(' ') {
                        // Register suffix as a class (if not already)
                        self.register_class(suffix);

                        // Update compound mapping
                        if let Some(suffix_obf) = self.classes.get(suffix).cloned() {
                            self.classes
                                .insert(original.clone(), format!("{prefix}{suffix_obf}"));
                        }
                        break; // longest match wins
                    }
                }
            }
        }
    }

    /// Generate a random obfuscated name.
    ///
    /// Names start with a letter (CSS requirement) followed by random alphanumerics.
    fn generate_name(&mut self) -> String {
        const FIRST: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
        const REST: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_";

        let len = self.rng.random_range(6..=10);
        let mut name = String::with_capacity(len);

        // Ensure uniqueness with counter prefix
        let idx = self.rng.random_range(0..FIRST.len());
        name.push(FIRST[idx] as char);

        for _ in 1..len {
            let idx = self.rng.random_range(0..REST.len());
            name.push(REST[idx] as char);
        }

        // Append counter to guarantee uniqueness
        name.push('_');
        name.push_str(&self.counter.to_string());
        self.counter += 1;

        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_with_seed() {
        let mut m1 = SymbolMap::new(Some(42));
        m1.register_class("foo");
        m1.register_id("bar");

        let mut m2 = SymbolMap::new(Some(42));
        m2.register_class("foo");
        m2.register_id("bar");

        assert_eq!(m1.get_class("foo"), m2.get_class("foo"));
        assert_eq!(m1.get_id("bar"), m2.get_id("bar"));
    }

    #[test]
    fn unique_names() {
        let mut m = SymbolMap::new(Some(0));
        m.register_class("a");
        m.register_class("b");
        m.register_id("c");

        let a = m.get_class("a").unwrap();
        let b = m.get_class("b").unwrap();
        let c = m.get_id("c").unwrap();

        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_ne!(b, c);
    }

    #[test]
    fn idempotent_register() {
        let mut m = SymbolMap::new(Some(0));
        m.register_class("foo");
        let first = m.get_class("foo").unwrap().to_owned();
        m.register_class("foo");
        assert_eq!(m.get_class("foo").unwrap(), first);
    }
}
