const MAX_DEPTH: usize = 64;
const MAX_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Node {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
    pub text: String,
}

impl Node {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }

    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> {
        self.children.iter().filter(move |c| c.name == name)
    }
}

pub fn parse(text: &str) -> Result<Node, String> {
    if text.len() > MAX_BYTES {
        return Err("the file is too large".into());
    }
    let mut reader = Reader {
        text,
        at: 0,
        depth: 0,
    };
    reader.skip_misc()?;
    let root = reader.element()?;
    reader.skip_misc()?;
    if reader.at < text.len() {
        return Err("text after the root element".into());
    }
    Ok(root)
}

struct Reader<'a> {
    text: &'a str,
    at: usize,
    depth: usize,
}

impl Reader<'_> {
    fn rest(&self) -> &str {
        &self.text[self.at..]
    }

    fn starts_with(&self, s: &str) -> bool {
        self.rest().starts_with(s)
    }

    fn skip_to_after(&mut self, end: &str) -> Result<(), String> {
        match self.rest().find(end) {
            Some(i) => {
                self.at += i + end.len();
                Ok(())
            }
            None => Err(format!("missing {end}")),
        }
    }

    fn skip_space(&mut self) {
        let trimmed = self.rest().trim_start();
        self.at = self.text.len() - trimmed.len();
    }

    fn skip_misc(&mut self) -> Result<(), String> {
        loop {
            self.skip_space();
            if self.starts_with("<?") {
                self.skip_to_after("?>")?;
            } else if self.starts_with("<!--") {
                self.skip_to_after("-->")?;
            } else if self.starts_with("<!DOCTYPE") {
                self.doctype()?;
            } else {
                return Ok(());
            }
        }
    }

    fn doctype(&mut self) -> Result<(), String> {
        let mut brackets = 0usize;
        for (i, c) in self.rest().char_indices() {
            match c {
                '[' => brackets += 1,
                ']' => brackets = brackets.saturating_sub(1),
                '>' if brackets == 0 => {
                    self.at += i + 1;
                    return Ok(());
                }
                _ => {}
            }
        }
        Err("unterminated DOCTYPE".into())
    }

    fn name(&mut self) -> Result<String, String> {
        let len = self
            .rest()
            .find(|c: char| !(c.is_alphanumeric() || matches!(c, ':' | '_' | '-' | '.')))
            .unwrap_or(self.rest().len());
        if len == 0 {
            return Err("expected a name".into());
        }
        let name = self.rest()[..len].to_owned();
        self.at += len;
        Ok(name)
    }

    fn element(&mut self) -> Result<Node, String> {
        if self.depth >= MAX_DEPTH {
            return Err("elements nest too deeply".into());
        }
        if !self.starts_with("<") {
            return Err("expected an element".into());
        }
        self.at += 1;
        let mut node = Node {
            name: self.name()?,
            ..Node::default()
        };
        loop {
            self.skip_space();
            if self.starts_with("/>") {
                self.at += 2;
                return Ok(node);
            }
            if self.starts_with(">") {
                self.at += 1;
                break;
            }
            let key = self.name()?;
            self.skip_space();
            if !self.starts_with("=") {
                return Err(format!("attribute {key} has no value"));
            }
            self.at += 1;
            self.skip_space();
            let quote = match self.rest().chars().next() {
                Some(q @ ('"' | '\'')) => q,
                _ => return Err(format!("attribute {key} isn't quoted")),
            };
            self.at += 1;
            let end = self
                .rest()
                .find(quote)
                .ok_or_else(|| format!("attribute {key} isn't closed"))?;
            let raw = &self.rest()[..end];
            node.attrs.push((key, decode(raw)?));
            self.at += end + 1;
        }
        self.depth += 1;
        let mut text = String::new();
        loop {
            if self.starts_with("</") {
                self.at += 2;
                let close = self.name()?;
                self.skip_space();
                if close != node.name || !self.starts_with(">") {
                    return Err(format!("</{close}> doesn't close <{}>", node.name));
                }
                self.at += 1;
                break;
            } else if self.starts_with("<!--") {
                self.skip_to_after("-->")?;
            } else if self.starts_with("<![CDATA[") {
                self.at += 9;
                let end = self.rest().find("]]>").ok_or("unterminated CDATA")?;
                text.push_str(&self.rest()[..end]);
                self.at += end + 3;
            } else if self.starts_with("<?") {
                self.skip_to_after("?>")?;
            } else if self.starts_with("<") {
                let child = self.element()?;
                node.children.push(child);
            } else if self.at >= self.text.len() {
                return Err(format!("<{}> is never closed", node.name));
            } else {
                let end = self.rest().find('<').unwrap_or(self.rest().len());
                text.push_str(&decode(&self.rest()[..end])?);
                self.at += end;
            }
        }
        self.depth -= 1;
        node.text = text.trim().to_owned();
        Ok(node)
    }
}

fn decode(raw: &str) -> Result<String, String> {
    if !raw.contains('&') {
        return Ok(raw.to_owned());
    }
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i + 1..];
        let end = rest.find(';').ok_or("an & with no ;")?;
        let entity = &rest[..end];
        match entity {
            "amp" => out.push('&'),
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            _ => {
                let number = entity.strip_prefix('#').ok_or("an unknown entity")?;
                let code = match number.strip_prefix('x') {
                    Some(hex) => u32::from_str_radix(hex, 16),
                    None => number.parse::<u32>(),
                }
                .map_err(|_| "a bad character reference")?;
                out.push(char::from_u32(code).ok_or("a bad character reference")?);
            }
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_elements_attributes_and_text() {
        let doc = r#"<?xml version="1.0"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<a x="1" y='2 &amp; 3'><!-- note --><b>one &lt;&#65;&#x42;&gt;</b><c/><![CDATA[<raw>]]></a>"#;
        let root = parse(doc).unwrap();
        assert_eq!(root.name, "a");
        assert_eq!(root.attr("x"), Some("1"));
        assert_eq!(root.attr("y"), Some("2 & 3"));
        assert_eq!(root.child("b").unwrap().text, "one <AB>");
        assert!(root.child("c").unwrap().children.is_empty());
        assert_eq!(root.text, "<raw>");
    }

    #[test]
    fn a_doctype_entity_is_not_resolved() {
        let doc = r#"<!DOCTYPE a [<!ENTITY x SYSTEM "file:///etc/passwd">]><a>&x;</a>"#;
        assert!(parse(doc).is_err(), "an unknown entity is refused");
        let plain = r#"<!DOCTYPE a [<!ENTITY x "y">]><a>ok</a>"#;
        assert_eq!(parse(plain).unwrap().text, "ok");
    }

    #[test]
    fn broken_documents_are_errors() {
        for bad in [
            "",
            "<a>",
            "<a></b>",
            "<a x=1></a>",
            "<a>&nope;</a>",
            "<a/><b/>",
            "<a>&#xZZ;</a>",
            "<a",
            "text",
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
        let deep = "<a>".repeat(200) + &"</a>".repeat(200);
        assert!(parse(&deep).is_err());
    }

    #[test]
    fn qualified_names_stay_as_written() {
        let root = parse(r#"<i xmlns:s="u"><s:v>5</s:v><e s:k="z"/></i>"#).unwrap();
        assert_eq!(root.child("s:v").unwrap().text, "5");
        assert_eq!(root.child("e").unwrap().attr("s:k"), Some("z"));
        assert_eq!(root.children_named("e").count(), 1);
    }
}
