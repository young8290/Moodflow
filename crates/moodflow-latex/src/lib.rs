//! A bounded symbol conversion, not a TeX interpreter. Unsupported syntax remains visible.
use anyhow::{Result, ensure};
use serde::Serialize;
use std::collections::BTreeSet;

pub const MAX_UNITS: usize = 16384;
#[derive(Debug, Serialize)]
pub struct Conversion {
    pub text: String,
    pub unsupported: Vec<String>,
}
struct Parser {
    input: Vec<char>,
    position: usize,
    unsupported: BTreeSet<String>,
}
impl Parser {
    fn group(&mut self, depth: usize, grouped: bool) -> Result<String> {
        ensure!(depth <= 32, "ai_tools.latex_limit");
        let mut out = String::new();
        while let Some(c) = self.input.get(self.position).copied() {
            self.position += 1;
            match c {
                '}' if grouped => return Ok(out),
                '}' => anyhow::bail!("ai_tools.latex_syntax"),
                '{' => out.push_str(&self.group(depth + 1, true)?),
                '\\' => {
                    let start = self.position;
                    while self
                        .input
                        .get(self.position)
                        .is_some_and(char::is_ascii_alphabetic)
                    {
                        self.position += 1;
                    }
                    if start == self.position {
                        let c = self
                            .input
                            .get(self.position)
                            .copied()
                            .ok_or_else(|| anyhow::anyhow!("ai_tools.latex_syntax"))?;
                        self.position += 1;
                        out.push(if c == ',' || c == ';' || c == ' ' {
                            ' '
                        } else {
                            c
                        });
                        continue;
                    }
                    let name: String = self.input[start..self.position].iter().collect();
                    if let Some(symbol) = symbol(&name) {
                        out.push_str(symbol);
                    } else if name == "frac" {
                        let a = self.argument(depth)?;
                        let b = self.argument(depth)?;
                        out.push_str(&format!("({a})/({b})"));
                    } else if name == "sqrt" {
                        let value = self.argument(depth)?;
                        out.push_str(&format!("√({value})"));
                    } else if matches!(
                        name.as_str(),
                        "text" | "mathrm" | "mathbf" | "mathit" | "operatorname"
                    ) {
                        out.push_str(&self.argument(depth)?);
                    } else {
                        ensure!(self.unsupported.len() < 256, "ai_tools.latex_limit");
                        self.unsupported.insert(format!("\\{name}"));
                        out.push('\\');
                        out.push_str(&name);
                    }
                }
                '^' | '_' => {
                    let argument = self.argument(depth)?;
                    let script: Option<String> =
                        argument.chars().map(|v| script(v, c == '^')).collect();
                    if let Some(script) = script {
                        out.push_str(&script);
                    } else {
                        out.push(c);
                        out.push('{');
                        out.push_str(&argument);
                        out.push('}');
                    }
                }
                _ => out.push(c),
            }
            ensure!(out.len() <= MAX_UNITS * 8, "ai_tools.latex_limit");
        }
        ensure!(!grouped, "ai_tools.latex_syntax");
        Ok(out)
    }
    fn argument(&mut self, depth: usize) -> Result<String> {
        while self
            .input
            .get(self.position)
            .is_some_and(|c| c.is_whitespace())
        {
            self.position += 1;
        }
        let c = self
            .input
            .get(self.position)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("ai_tools.latex_syntax"))?;
        self.position += 1;
        if c == '{' {
            self.group(depth + 1, true)
        } else {
            ensure!(c != '}' && c != '\\', "ai_tools.latex_syntax");
            Ok(c.to_string())
        }
    }
}
fn script(c: char, upper: bool) -> Option<char> {
    let plain = "0123456789+-=()in";
    let mapped = if upper {
        "⁰¹²³⁴⁵⁶⁷⁸⁹⁺⁻⁼⁽⁾ⁱⁿ"
    } else {
        "₀₁₂₃₄₅₆₇₈₉₊₋₌₍₎ᵢₙ"
    };
    plain
        .chars()
        .position(|p| p == c)
        .and_then(|i| mapped.chars().nth(i))
}
fn symbol(name: &str) -> Option<&'static str> {
    Some(match name {
        "alpha" => "α",
        "beta" => "β",
        "gamma" => "γ",
        "delta" => "δ",
        "epsilon" => "ε",
        "varepsilon" => "ϵ",
        "zeta" => "ζ",
        "eta" => "η",
        "theta" => "θ",
        "vartheta" => "ϑ",
        "iota" => "ι",
        "kappa" => "κ",
        "lambda" => "λ",
        "mu" => "μ",
        "nu" => "ν",
        "xi" => "ξ",
        "pi" => "π",
        "rho" => "ρ",
        "sigma" => "σ",
        "tau" => "τ",
        "upsilon" => "υ",
        "phi" => "φ",
        "varphi" => "ϕ",
        "chi" => "χ",
        "psi" => "ψ",
        "omega" => "ω",
        "Gamma" => "Γ",
        "Delta" => "Δ",
        "Theta" => "Θ",
        "Lambda" => "Λ",
        "Xi" => "Ξ",
        "Pi" => "Π",
        "Sigma" => "Σ",
        "Upsilon" => "Υ",
        "Phi" => "Φ",
        "Psi" => "Ψ",
        "Omega" => "Ω",
        "times" => "×",
        "cdot" => "⋅",
        "div" => "÷",
        "pm" => "±",
        "mp" => "∓",
        "le" | "leq" => "≤",
        "ge" | "geq" => "≥",
        "ne" | "neq" => "≠",
        "approx" => "≈",
        "equiv" => "≡",
        "propto" => "∝",
        "infty" => "∞",
        "partial" => "∂",
        "nabla" => "∇",
        "sum" => "∑",
        "prod" => "∏",
        "int" => "∫",
        "iint" => "∬",
        "oint" => "∮",
        "in" => "∈",
        "notin" => "∉",
        "subset" => "⊂",
        "subseteq" => "⊆",
        "supset" => "⊃",
        "cup" => "∪",
        "cap" => "∩",
        "emptyset" => "∅",
        "forall" => "∀",
        "exists" => "∃",
        "neg" => "¬",
        "land" => "∧",
        "lor" => "∨",
        "to" | "rightarrow" => "→",
        "leftarrow" => "←",
        "leftrightarrow" => "↔",
        "Rightarrow" => "⇒",
        "Leftarrow" => "⇐",
        "Leftrightarrow" => "⇔",
        "ldots" => "…",
        "cdots" => "⋯",
        "degree" => "°",
        "quad" | "qquad" => " ",
        "left" | "right" => "",
        _ => return None,
    })
}
pub fn convert(input: &str) -> Result<Conversion> {
    ensure!(
        input.encode_utf16().count() <= MAX_UNITS,
        "ai_tools.latex_limit"
    );
    let mut parser = Parser {
        input: input.chars().collect(),
        position: 0,
        unsupported: BTreeSet::new(),
    };
    let text = parser.group(0, false)?;
    ensure!(
        text.encode_utf16().count() <= MAX_UNITS,
        "ai_tools.latex_limit"
    );
    Ok(Conversion {
        text,
        unsupported: parser.unsupported.into_iter().collect(),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn symbols_fractions_scripts_and_unicode_text() {
        assert_eq!(convert(r"\alpha + \beta").unwrap().text, "α + β");
        assert_eq!(
            convert(r"\frac{1}{n} + x^{12} + a_2 \leq \sqrt{2}")
                .unwrap()
                .text,
            "(1)/(n) + x¹² + a₂ ≤ √(2)"
        );
        assert_eq!(
            convert(r"\text{面积} = \pi r^2").unwrap().text,
            "面积 = π r²"
        );
    }
    #[test]
    fn unsupported_commands_remain_visible_and_are_not_executed() {
        let value = convert(r"\input{/etc/passwd} + \unknown{x}").unwrap();
        assert!(value.text.contains(r"\input/etc/passwd"));
        assert_eq!(value.unsupported, vec![r"\input", r"\unknown"]);
        assert_eq!(convert("x^{abc}").unwrap().text, "x^{abc}");
    }
    #[test]
    fn bounds_and_malformed_input_do_not_return_partial_results() {
        for input in [
            "a}".into(),
            "{x".into(),
            "x^".into(),
            format!("{}x{}", "{".repeat(33), "}".repeat(33)),
            "😀".repeat(8193),
        ] {
            assert!(convert(&input).is_err());
        }
    }
}
