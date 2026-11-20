//! Campo de texto de uma linha (a omnibox). Posições são índices em bytes, sempre em
//! fronteira de caractere.

#[derive(Default, Debug, Clone)]
pub struct TextField {
    pub text: String,
    pub cursor: usize,
    /// Início da seleção (a outra ponta é o cursor).
    pub anchor: Option<usize>,
    pub focused: bool,
}

impl TextField {
    /// Foca mostrando `text` todo selecionado (como ao clicar na barra de endereço).
    pub fn focus_with(&mut self, text: &str) {
        self.text = text.to_owned();
        self.cursor = self.text.len();
        self.anchor = Some(0);
        self.focused = true;
    }

    pub fn blur(&mut self) {
        self.focused = false;
        self.anchor = None;
    }

    pub fn selection(&self) -> Option<(usize, usize)> {
        let a = self.anchor?;
        (a != self.cursor).then(|| (a.min(self.cursor), a.max(self.cursor)))
    }

    pub fn selected_text(&self) -> Option<&str> {
        self.selection().map(|(a, b)| &self.text[a..b])
    }

    pub fn delete_selection(&mut self) -> bool {
        match self.selection() {
            Some((a, b)) => {
                self.text.replace_range(a..b, "");
                self.cursor = a;
                self.anchor = None;
                true
            }
            None => {
                self.anchor = None;
                false
            }
        }
    }

    pub fn insert(&mut self, s: &str) {
        self.delete_selection();
        self.text.insert_str(self.cursor, s);
        self.cursor += s.len();
    }

    pub fn backspace(&mut self) {
        if !self.delete_selection() && self.cursor > 0 {
            let prev = self.prev_boundary(self.cursor);
            self.text.replace_range(prev..self.cursor, "");
            self.cursor = prev;
        }
    }

    pub fn delete(&mut self) {
        if !self.delete_selection() && self.cursor < self.text.len() {
            let next = self.next_boundary(self.cursor);
            self.text.replace_range(self.cursor..next, "");
        }
    }

    pub fn set_cursor(&mut self, pos: usize, extend: bool) {
        if extend {
            self.anchor.get_or_insert(self.cursor);
        } else {
            self.anchor = None;
        }
        self.cursor = pos.min(self.text.len());
    }

    pub fn move_left(&mut self, shift: bool, word: bool) {
        if !shift && let Some((a, _)) = self.selection() {
            self.set_cursor(a, false);
            return;
        }
        let pos = if word {
            self.word_left(self.cursor)
        } else {
            self.prev_boundary(self.cursor)
        };
        self.set_cursor(pos, shift);
    }

    pub fn move_right(&mut self, shift: bool, word: bool) {
        if !shift && let Some((_, b)) = self.selection() {
            self.set_cursor(b, false);
            return;
        }
        let pos = if word {
            self.word_right(self.cursor)
        } else {
            self.next_boundary(self.cursor)
        };
        self.set_cursor(pos, shift);
    }

    pub fn home(&mut self, shift: bool) {
        self.set_cursor(0, shift);
    }

    pub fn end(&mut self, shift: bool) {
        self.set_cursor(self.text.len(), shift);
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.cursor = self.text.len();
    }

    fn prev_boundary(&self, i: usize) -> usize {
        self.text[..i]
            .char_indices()
            .next_back()
            .map(|(p, _)| p)
            .unwrap_or(0)
    }

    fn next_boundary(&self, i: usize) -> usize {
        self.text[i..]
            .chars()
            .next()
            .map(|c| i + c.len_utf8())
            .unwrap_or(i)
    }

    fn is_word(c: char) -> bool {
        c.is_alphanumeric() || c == '_'
    }

    fn word_left(&self, mut i: usize) -> usize {
        while i > 0
            && !self.text[..i]
                .chars()
                .next_back()
                .is_some_and(Self::is_word)
        {
            i = self.prev_boundary(i);
        }
        while i > 0
            && self.text[..i]
                .chars()
                .next_back()
                .is_some_and(Self::is_word)
        {
            i = self.prev_boundary(i);
        }
        i
    }

    fn word_right(&self, mut i: usize) -> usize {
        let len = self.text.len();
        while i < len && !self.text[i..].chars().next().is_some_and(Self::is_word) {
            i = self.next_boundary(i);
        }
        while i < len && self.text[i..].chars().next().is_some_and(Self::is_word) {
            i = self.next_boundary(i);
        }
        i
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edicao() {
        let mut f = TextField::default();
        f.focus_with("nous.dev");
        f.insert("ação");
        assert_eq!(f.text, "ação");
        f.backspace();
        assert_eq!(f.text, "açã");
        f.move_left(false, false);
        f.move_left(true, false);
        assert_eq!(f.selected_text(), Some("ç"));
        f.insert("X");
        assert_eq!(f.text, "aXã");
        f.home(false);
        f.delete();
        assert_eq!(f.text, "Xã");
        f.select_all();
        f.backspace();
        assert_eq!(f.text, "");
    }

    #[test]
    fn palavras() {
        let mut f = TextField::default();
        f.focus_with("https://pt.wikipedia.org");
        f.end(false);
        f.move_left(false, true);
        assert_eq!(&f.text[f.cursor..], "org");
        f.move_left(false, true);
        assert_eq!(&f.text[f.cursor..], "wikipedia.org");
    }
}
