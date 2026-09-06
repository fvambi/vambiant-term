//! Commands that hand a command line to another shell: the line is read
//! and classified too, or, when it comes from an expansion, is obfuscated.

use super::finding;
use crate::classify::{Context, Finding, SafetyClass, classify_program};
use crate::shell::{self, Simple, Word};

pub(super) fn nested(line: &str, ctx: &Context, findings: &mut Vec<Finding>, depth: usize) {
    match shell::parse(line) {
        Ok(p) => {
            classify_program(&p, ctx, findings, depth + 1);
        }
        Err(e) => findings.push(finding(
            SafetyClass::Unparseable,
            "parse-failed",
            line.chars().take(80).collect::<String>(),
            e.to_string(),
        )),
    }
}

pub(super) fn eval(
    cmd: &Simple,
    list: &[String],
    ctx: &Context,
    findings: &mut Vec<Finding>,
    depth: usize,
) {
    if cmd.words.iter().skip(1).any(Word::has_expansion) {
        findings.push(finding(
            SafetyClass::Obfuscated,
            "eval-expansion",
            format!("eval {}", list.join(" ")),
            "evaluates text that comes from a variable or substitution",
        ));
    } else {
        nested(&list.join(" "), ctx, findings, depth);
    }
}

pub(super) fn shell_c(
    cmd: &Simple,
    list: &[String],
    ctx: &Context,
    findings: &mut Vec<Finding>,
    depth: usize,
) {
    let Some(i) = list
        .iter()
        .position(|a| a == "-c" || (a.starts_with('-') && !a.starts_with("--") && a.contains('c')))
    else {
        // `bash <(curl …)` or `sh script`: a process substitution is a download run blind.
        if cmd
            .words
            .iter()
            .skip(1)
            .any(|w| w.substitutions().next().is_some())
        {
            findings.push(finding(
                SafetyClass::Obfuscated,
                "shell-from-substitution",
                cmd.words[0].text(),
                "a shell runs the output of another command unseen",
            ));
        }
        return;
    };
    let Some(code) = cmd.words.get(i + 2) else {
        return;
    };
    match code.literal() {
        Some(text) => nested(&text, ctx, findings, depth),
        None => findings.push(finding(
            SafetyClass::Obfuscated,
            "shell-c-expansion",
            code.text(),
            "the script text comes from a variable or substitution",
        )),
    }
}

pub(super) fn inline_code(name: &str, list: &[String], findings: &mut Vec<Finding>) {
    let Some(i) = list
        .iter()
        .position(|a| a == "-c" || a == "-e" || a == "-r")
    else {
        return;
    };
    let code = list.get(i + 1).map_or("", String::as_str);
    let suspicious = [
        "base64",
        "b64decode",
        "exec(",
        "eval(",
        "os.system",
        "subprocess",
        "child_process",
        "\\x",
    ];
    if suspicious.iter().any(|s| code.contains(s)) {
        findings.push(finding(
            SafetyClass::Obfuscated,
            "inline-code",
            format!("{name} {}", list[i]),
            "inline code decodes or executes text at runtime",
        ));
    }
}
