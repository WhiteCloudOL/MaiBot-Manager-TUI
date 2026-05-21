use dialoguer::theme::{ColorfulTheme, Theme};
use std::fmt;

/// Confirm 提示的 yes/no 改为更简短的 y/n。其他方法全部委托给内部的
/// ColorfulTheme，保留官方默认配色。
pub(crate) struct AppTheme {
    inner: ColorfulTheme,
}

impl AppTheme {
    pub(crate) fn new() -> Self {
        Self {
            inner: ColorfulTheme::default(),
        }
    }
}

impl Theme for AppTheme {
    fn format_prompt(&self, f: &mut dyn fmt::Write, prompt: &str) -> fmt::Result {
        self.inner.format_prompt(f, prompt)
    }

    fn format_error(&self, f: &mut dyn fmt::Write, err: &str) -> fmt::Result {
        self.inner.format_error(f, err)
    }

    fn format_confirm_prompt(
        &self,
        f: &mut dyn fmt::Write,
        prompt: &str,
        default: Option<bool>,
    ) -> fmt::Result {
        if !prompt.is_empty() {
            write!(
                f,
                "{} {} ",
                &self.inner.prompt_prefix,
                self.inner.prompt_style.apply_to(prompt)
            )?;
        }
        match default {
            None => write!(
                f,
                "{} {}",
                self.inner.hint_style.apply_to("(y/n)"),
                &self.inner.prompt_suffix
            ),
            Some(true) => write!(
                f,
                "{} {} {}",
                self.inner.hint_style.apply_to("(y/n)"),
                &self.inner.prompt_suffix,
                self.inner.defaults_style.apply_to("y")
            ),
            Some(false) => write!(
                f,
                "{} {} {}",
                self.inner.hint_style.apply_to("(y/n)"),
                &self.inner.prompt_suffix,
                self.inner.defaults_style.apply_to("n")
            ),
        }
    }

    fn format_confirm_prompt_selection(
        &self,
        f: &mut dyn fmt::Write,
        prompt: &str,
        selection: Option<bool>,
    ) -> fmt::Result {
        if !prompt.is_empty() {
            write!(
                f,
                "{} {} ",
                &self.inner.success_prefix,
                self.inner.prompt_style.apply_to(prompt)
            )?;
        }
        let selection = selection.map(|b| if b { "y" } else { "n" });
        match selection {
            Some(sel) => write!(
                f,
                "{} {}",
                &self.inner.success_suffix,
                self.inner.values_style.apply_to(sel)
            ),
            None => write!(f, "{}", &self.inner.success_suffix),
        }
    }

    fn format_input_prompt(
        &self,
        f: &mut dyn fmt::Write,
        prompt: &str,
        default: Option<&str>,
    ) -> fmt::Result {
        self.inner.format_input_prompt(f, prompt, default)
    }

    fn format_input_prompt_selection(
        &self,
        f: &mut dyn fmt::Write,
        prompt: &str,
        sel: &str,
    ) -> fmt::Result {
        self.inner.format_input_prompt_selection(f, prompt, sel)
    }

    fn format_select_prompt(&self, f: &mut dyn fmt::Write, prompt: &str) -> fmt::Result {
        self.inner.format_select_prompt(f, prompt)
    }

    fn format_select_prompt_selection(
        &self,
        f: &mut dyn fmt::Write,
        prompt: &str,
        sel: &str,
    ) -> fmt::Result {
        self.inner.format_select_prompt_selection(f, prompt, sel)
    }

    fn format_multi_select_prompt(&self, f: &mut dyn fmt::Write, prompt: &str) -> fmt::Result {
        self.inner.format_multi_select_prompt(f, prompt)
    }

    fn format_sort_prompt(&self, f: &mut dyn fmt::Write, prompt: &str) -> fmt::Result {
        self.inner.format_sort_prompt(f, prompt)
    }

    fn format_multi_select_prompt_selection(
        &self,
        f: &mut dyn fmt::Write,
        prompt: &str,
        selections: &[&str],
    ) -> fmt::Result {
        self.inner
            .format_multi_select_prompt_selection(f, prompt, selections)
    }

    fn format_sort_prompt_selection(
        &self,
        f: &mut dyn fmt::Write,
        prompt: &str,
        selections: &[&str],
    ) -> fmt::Result {
        self.inner
            .format_sort_prompt_selection(f, prompt, selections)
    }

    fn format_select_prompt_item(
        &self,
        f: &mut dyn fmt::Write,
        text: &str,
        active: bool,
    ) -> fmt::Result {
        self.inner.format_select_prompt_item(f, text, active)
    }

    fn format_multi_select_prompt_item(
        &self,
        f: &mut dyn fmt::Write,
        text: &str,
        checked: bool,
        active: bool,
    ) -> fmt::Result {
        self.inner
            .format_multi_select_prompt_item(f, text, checked, active)
    }

    fn format_sort_prompt_item(
        &self,
        f: &mut dyn fmt::Write,
        text: &str,
        picked: bool,
        active: bool,
    ) -> fmt::Result {
        self.inner
            .format_sort_prompt_item(f, text, picked, active)
    }
}
