// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use std::{
    collections::BTreeMap,
    fs,
    iter::Peekable,
    num::NonZeroU32,
    path::{Path, PathBuf},
    str::CharIndices,
};

use t32_language_server_serde::{
    PracticeFuncArgumentPattern, PracticeFuncNamePattern, PracticeFunctionArguments,
    PracticeFunctionDefinition, PracticeFunctionName,
};

use crate::ReturnCode;

#[derive(Debug)]
struct PracticeFunctionIndex {
    names: Vec<String>,
    operations: Vec<String>,
}

struct Doc<'a> {
    man: &'a str,
    copyright: &'a str,
}

const DOCS: [Doc; 2] = [
    Doc {
        man: "general_func.pdf.man.txt",
        copyright: "general_func.pdf.copyright.txt",
    },
    Doc {
        man: "ide_func.pdf.man.txt",
        copyright: "ide_func.pdf.copyright.txt",
    },
];

const MAN_SUFFIX: &'static str = ".man.txt";
const MAJOR_INDEX_SEC: &'static str = ".............";

#[allow(unused)]
const FORM_FEED: char = '\x0C';

impl PracticeFunctionIndex {
    pub fn build(names: Vec<String>, operations: Vec<String>) -> Self {
        debug_assert!(!names.is_empty());
        debug_assert_eq!(names.len(), operations.len());

        Self { names, operations }
    }
}

pub fn extract_func_defs(dir: &Path) -> Result<Vec<PracticeFunctionDefinition>, ReturnCode> {
    let mut files: Vec<(PathBuf, PathBuf, String)> = Vec::new();
    for doc in DOCS {
        let man = dir.join(doc.man);

        if !man.is_file() {
            eprintln!(
                "ERROR: Function reference document \"{}\" not found.",
                man.display()
            );
            return Err(ReturnCode::NoInputErr);
        }

        let copyright = dir.join(doc.copyright);

        if !copyright.is_file() {
            eprintln!(
                "ERROR: Function reference document \"{}\" not found.",
                copyright.display()
            );
            return Err(ReturnCode::NoInputErr);
        }
        files.push((man, copyright, doc.man.replace(MAN_SUFFIX, "")))
    }

    let mut defs: Vec<PracticeFunctionDefinition> = Vec::new();
    for (man, copyright, source) in files.into_iter() {
        let bytes = match fs::read(&man) {
            Ok(b) => b,
            Err(err) => {
                eprintln!("ERROR: Cannot read file \"{}\": {}", man.display(), err);
                return Err(ReturnCode::NoInputErr);
            }
        };

        let text = match str::from_utf8(&bytes) {
            Ok(t) => t,
            Err(err) => {
                eprintln!("ERROR: Cannot decode file \"{}\": {}", man.display(), err);
                return Err(ReturnCode::NoInputErr);
            }
        };

        let index: PracticeFunctionIndex = parse_index(&text);
        let desc = parse_func_descriptions(&text);

        let bytes = match fs::read(&copyright) {
            Ok(b) => b,
            Err(err) => {
                eprintln!(
                    "ERROR: Cannot read file \"{}\": {}",
                    copyright.display(),
                    err
                );
                return Err(ReturnCode::NoInputErr);
            }
        };

        let text = match str::from_utf8(&bytes) {
            Ok(t) => t,
            Err(err) => {
                eprintln!(
                    "ERROR: Cannot decode file \"{}\": {}",
                    copyright.display(),
                    err
                );
                return Err(ReturnCode::NoInputErr);
            }
        };
        let cr = parse_copyright(&text);

        defs.append(&mut create_func_definitions(index, desc, cr, source));
    }
    Ok(defs)
}

fn create_func_definitions(
    index: PracticeFunctionIndex,
    desc: Vec<(PracticeFunctionName, PracticeFunctionArguments)>,
    copyright: String,
    source: String,
) -> Vec<PracticeFunctionDefinition> {
    let mut map: BTreeMap<String, String> = BTreeMap::new();
    for (name, operation) in index.names.into_iter().zip(index.operations.into_iter()) {
        let name = fix_typos(name);
        map.insert(name, operation);
    }

    let mut defs: Vec<PracticeFunctionDefinition> = Vec::with_capacity(desc.len());
    for (name, args) in desc {
        let entry = map_alias_variants_to_index(name.to_string());

        let operation = map.get(&entry);
        if operation.is_none() {
            panic!("There is no index entry for function \"{}\".", name);
        }

        defs.push(PracticeFunctionDefinition {
            name,
            args,
            operation: operation.unwrap().clone(),
            copyright: copyright.clone(),
            source: source.clone(),
        });
    }
    defs
}

fn parse_index(text: &str) -> PracticeFunctionIndex {
    let mut entered_index = false;
    let mut left_index = false;

    let mut funcs: Vec<String> = Vec::new();
    let mut operations: Vec<String> = Vec::new();

    for line in text.lines() {
        if entered_index {
            match parse_func_entry(line) {
                (Some(name), Some(op)) => {
                    funcs.push(name.to_string());
                    operations.push(op.to_string());
                }
                (Some(name), None) => {
                    let operation = supplement_missing_operational_desc(name);
                    if let Some(op) = operation {
                        funcs.push(name.to_string());
                        operations.push(op.to_string());
                    }
                }
                _ => (),
            }
        }

        if !entered_index && line.contains("TRACE32 Documents") {
            entered_index = true;
        } else if !left_index && line.contains("History") && !line.contains(MAJOR_INDEX_SEC) {
            left_index = true;
        }

        if left_index {
            break;
        }
    }

    PracticeFunctionIndex::build(funcs, operations)
}

fn parse_copyright(text: &str) -> String {
    for line in text.lines() {
        let mut chars = line.char_indices();
        loop {
            match chars.next() {
                Some((idx, ch)) => {
                    if ch == '©' {
                        if !year(&mut chars) {
                            break;
                        }

                        let Some((_, '-')) = chars.next() else {
                            break;
                        };

                        if !year(&mut chars) {
                            break;
                        }

                        let Some((_, ' ')) = chars.next() else {
                            break;
                        };

                        let Some(end) = word(&mut chars) else {
                            break;
                        };
                        return line[idx..end].to_string();
                    }
                }
                None => break,
            }
        }
    }
    panic!("Cannot detect document copyright.");
}

fn parse_func_descriptions(text: &str) -> Vec<(PracticeFunctionName, PracticeFunctionArguments)> {
    let mut multiline: Option<&str> = None;

    let mut desc: Vec<(PracticeFunctionName, PracticeFunctionArguments)> = Vec::new();

    for line in text.lines() {
        if line.contains("Syntax:")
            || line.contains("Syntax 1:    ")
            || line.contains("Syntax 2:    ")
            || multiline.is_some()
        {
            if line.ends_with('\\') {
                multiline = Some(line);
                continue;
            }

            let signature = if let Some(first) = multiline {
                let mut full = String::from(first).replace(" \\", "");
                full.push_str(&line.replace(" ", ""));

                parse_signature_line(&full)
            } else {
                parse_signature_line(&line)
            };
            multiline = None;

            let Some((name, args)) = signature else {
                panic!("Cannot parse function signature.");
            };
            desc.push((name, args));
        }
    }
    desc
}

fn parse_signature_line(line: &str) -> Option<(PracticeFunctionName, PracticeFunctionArguments)> {
    let mut chars: Peekable<CharIndices> = line.char_indices().peekable();

    if !syntax_desc(line, &mut chars) {
        return None;
    }
    signature(line, &mut chars)
}

fn fix_typos(name: String) -> String {
    if name == "CONvert.BOOLTOINT" {
        "CONVert.BOOLTOINT".to_string()
    } else {
        name
    }
}

fn supplement_missing_operational_desc(name: &str) -> Option<String> {
    if name == "Integrator.ANALOG" {
        Some("Connection status of Analog Probe".to_string())
    } else if name == "IProbe.PROBE" {
        Some("Connection status of PROBE connector".to_string())
    } else if name == "PORTANALYZER" {
        Some("Connection status of port analyzer hardware".to_string())
    } else if name == "RunTime.ACTUAL" {
        Some("Value of actual RunTime column".to_string())
    } else if name == "RunTime.LAST" {
        Some("Value of laststart RunTime column".to_string())
    } else if name == "RunTime.LASTRUN" {
        Some("Time of last program run".to_string())
    } else if name == "RunTime.REFA" {
        Some("Value of ref A RunTime column".to_string())
    } else if name == "RunTime.REFB" {
        Some("Value of ref B RunTime column".to_string())
    } else if name == "STATE.HALT" {
        Some("State of the “halt” display".to_string())
    } else if name == "STATE.OSLK" {
        Some("State of the OS-lock bit".to_string())
    } else if name == "STATE.POWER" {
        Some("State of the target power line".to_string())
    } else if name == "STATE.RESET" {
        Some("State of the target reset line".to_string())
    } else if name == "STATE.RUN" {
        Some("Run state of CPU".to_string())
    } else if name == "SYStem.CADIconfig.RemoteServer" {
        Some("Connection information of the CADI server".to_string())
    } else if name == "SYStem.CADIconfig.Traceconfig" {
        Some("Connection information of the CADI trace plug-in".to_string())
    } else if name == "SYStem.CONFIG.<tap_position>" {
        Some("JTAG PRE and POST settings".to_string())
    } else if name == "SYStem.CONFIG.DEBUGPORT" {
        Some("Selected debug port".to_string())
    } else if name == "SYStem.CONFIG.DEBUGPORTTYPE" {
        Some("Selected debug port type".to_string())
    } else if name == "SYStem.CONFIG.ListCORE" {
        Some("Core list of the virtual target platform".to_string())
    } else if name == "SYStem.CONFIG.ListSIM" {
        Some("Simulation list of the virtual target platform".to_string())
    } else if name == "SYStem.CONFIG.Slave" {
        Some("State of SYStem.CONFIG Slave".to_string())
    } else if name == "SYStem.CONFIG.TAPState" {
        Some("Default JTAG TAP state".to_string())
    } else if name == "SYStem.HOOK" {
        Some("Address of the hook function".to_string())
    } else if name == "SYStem.IMASKASM" {
        Some("Interrupts status during ASM stepping".to_string())
    } else if name == "SYStem.IMASKHLL" {
        Some("Interrupts status during HLL stepping".to_string())
    } else if name == "SYStem.IRISconfig.RemoteServer" {
        Some("Connection information of the IRIS server".to_string())
    } else if name == "SYStem.JtagClock" {
        Some("JTAG clock value".to_string())
    } else if name == "SYStem.LittleEndian" {
        Some("Core runs in little endian mode".to_string())
    } else if name == "SYStem.MCDCommand.ResultString" {
        Some("Result of last MCD command".to_string())
    } else if name == "SYStem.MCDconfig.LIBrary" {
        Some("Path of MCD library".to_string())
    } else if name == "SYStem.Mode" {
        Some("State of debugger".to_string())
    } else if name == "SYStem.USECORE" {
        Some("CORE= value of the PBI= config section".to_string())
    } else if name == "SYStem.USEMASK" {
        Some("USEMASK of PowerView GUI".to_string())
    } else {
        None
    }
}

fn map_alias_variants_to_index(name: String) -> String {
    match &name {
        n if *n == "FORMAT.UDecimal" => "FORMAT.UDECIMAL".to_string(),
        n if *n == "MMU.DEFAULTPT.ZONE" => "MMU.DEFAULTPT".to_string(),
        n if *n == "MMU.DEFAULTTRANS.<range>.ZONE" => "MMU.DEFAULTTRANS.<range>".to_string(),
        n if *n == "MMU.FORMAT.ZONE" => "MMU.FORMAT".to_string(),
        n if *n == "STATE.PROCESSOR" => "SYStem.CPU".to_string(),
        n if *n == "TRANS.LIST.NUMBER.ZONE" => "TRANS.LIST.NUMBER".to_string(),
        n if *n == "TRANS.LIST.LOGRANGE.ZONE" => "TRANS.LIST.LOGRANGE".to_string(),
        n if *n == "TRANS.LIST.PHYSADDR.ZONE" => "TRANS.LIST.PHYSADDR".to_string(),
        n if *n == "TRANS.LIST.TYPE.ZONE" => "TRANS.LIST.TYPE".to_string(),
        n if *n == "TRANS.INTERMEDIATEEX" => "TRANS.INTERMEDIATE".to_string(),
        n if *n == "TRANS.INTERMEDIATEEX.VALID" => "TRANS.INTERMEDIATE.VALID".to_string(),
        n if *n == "TRANS.LINEAREX" => "TRANS.LINEAR".to_string(),
        n if *n == "TRANS.LINEAREX.VALID" => "TRANS.LINEAR.VALID".to_string(),
        n if *n == "TRANS.PHYSICALEX" => "TRANS.PHYSICAL".to_string(),
        n if *n == "TRANS.PHYSICALEX.VALID" => "TRANS.PHYSICAL.VALID".to_string(),
        _ => name,
    }
}

fn parse_func_entry<'a>(line: &'a str) -> (Option<&'a str>, Option<&'a str>) {
    let mut chars: Peekable<CharIndices> = line.char_indices().peekable();

    let mut func: Option<&str> = None;
    let mut operation: Option<&str> = None;
    let mut page = false;

    loop {
        if func.is_none() {
            match chars.next() {
                Some((idx, ch)) => match ch {
                    ch if ch.is_ascii_alphabetic() || ch == '_' => {
                        let Some(end) = func_name(&mut chars) else {
                            return (None, None);
                        };
                        func = Some(&line[idx..end]);
                    }
                    '.' => {
                        if main_section(&mut chars) {
                            return (None, None);
                        }
                    }
                    _ => (),
                },
                None => break,
            }
        } else if operation.is_none() {
            match chars.next() {
                Some((idx, ch)) => match ch {
                    '.' => {
                        if main_section(&mut chars) {
                            return (None, None);
                        }
                    }
                    ch if ch.is_ascii_alphanumeric() => {
                        let Some(end) = func_operation(&mut chars) else {
                            return (func, None);
                        };
                        operation = Some(&line[idx..end]);
                    }
                    _ => (),
                },
                None => break,
            }
        } else if !page {
            match chars.next() {
                Some((_, ch)) => match ch {
                    ch if ch.is_numeric() => {
                        page_num(&mut chars);
                        page = true;
                    }
                    ' ' => (),
                    _ => {
                        return (None, None);
                    }
                },
                None => break,
            }
        } else {
            match chars.next() {
                Some((_, ch)) => match ch {
                    ' ' => (),
                    _ => {
                        return (None, None);
                    }
                },
                None => break,
            }
        }
    }
    (func, operation)
}

fn func_name(chars: &mut Peekable<CharIndices>) -> Option<usize> {
    while let Some((idx, ch)) = chars.next() {
        match ch {
            ch if ch == '(' => return Some(idx),
            ch if !(ch.is_ascii_alphanumeric() || ['.', '_', '<', '>'].contains(&ch)) => break,
            _ => (),
        }
    }
    None
}

fn func_operation(chars: &mut Peekable<CharIndices>) -> Option<usize> {
    while let Some((idx, ch)) = chars.next() {
        if ch == ' '
            && let Some((_, next)) = chars.peek()
            && *next == ' '
        {
            return Some(idx);
        }
    }
    None
}

fn main_section(chars: &mut Peekable<CharIndices>) -> bool {
    let mut num: u32 = 0;

    while let Some((_, ch)) = chars.next() {
        if ch == '.' {
            num += 1;
        } else {
            break;
        }
    }
    num > 3
}

fn page_num(chars: &mut Peekable<CharIndices>) {
    while let Some((_, ch)) = chars.next() {
        if !ch.is_numeric() {
            break;
        }
    }
}

fn syntax_desc(line: &str, chars: &mut Peekable<CharIndices>) -> bool {
    loop {
        match chars.peek() {
            Some((idx, ch)) => match ch {
                ch if *ch == 'S' => {
                    if peek_word("Syntax:", &line[*idx..]) {
                        consumen("Syntax:".len() as u32, chars);
                        return true;
                    } else if peek_word("Syntax 1:     ", &line[*idx..]) {
                        consumen("Syntax 2:    ".len() as u32, chars);
                        return true;
                    } else if peek_word("Syntax 2:    ", &line[*idx..]) {
                        consumen("Syntax 2:    ".len() as u32, chars);
                        return true;
                    } else {
                        return false;
                    }
                }
                _ => (),
            },
            None => break,
        }
        let _ = chars.next();
    }
    return false;
}

fn signature(
    line: &str,
    chars: &mut Peekable<CharIndices>,
) -> Option<(PracticeFunctionName, PracticeFunctionArguments)> {
    loop {
        match chars.peek() {
            Some((_, ch)) => match ch {
                ch if ch.is_ascii_alphanumeric() || *ch == '_' => {
                    let Ok(name) = func_name_pattern(line, chars) else {
                        panic!("Cannot parse function name patterns.");
                    };
                    let name = expand_name_patterns(name);

                    let Ok(args) = args_patterns(line, chars) else {
                        panic!("Cannot parse function argument patters.");
                    };

                    return Some((
                        PracticeFunctionName::from_parts(name),
                        PracticeFunctionArguments::from_params(args),
                    ));
                }
                _ => (),
            },
            None => break,
        }
        chars.next();
    }
    None
}

fn expand_name_patterns(mut name: Vec<PracticeFuncNamePattern>) -> Vec<PracticeFuncNamePattern> {
    if name.len() == 2 {
        if let PracticeFuncNamePattern::Literal(pat) = &name[0]
            && pat == "DAP"
            && let PracticeFuncNamePattern::Literal(pat) = &name[1]
            && pat.starts_with("USER")
        {
            name[1] = PracticeFuncNamePattern::Chained(vec![
                PracticeFuncNamePattern::Literal("USER".to_string()),
                PracticeFuncNamePattern::Angled("x".to_string()),
            ])
        }
        name
    } else {
        name
    }
}

fn func_name_pattern(
    line: &str,
    chars: &mut Peekable<CharIndices>,
) -> Result<Vec<PracticeFuncNamePattern>, ()> {
    let mut components: Vec<PracticeFuncNamePattern> = Vec::new();

    loop {
        match chars.peek() {
            Some((idx, ch)) => match ch {
                '<' => {
                    let Some(pat) = angled_name_pattern(line, chars) else {
                        return Err(());
                    };
                    components.push(pat);
                }
                ch if ch.is_ascii_alphanumeric() || *ch == '_' => {
                    let start = *idx;

                    let Some(end) = pattern_name(chars) else {
                        return Err(());
                    };
                    let pat = PracticeFuncNamePattern::Literal(String::from(&line[start..=end]));
                    components.push(pat);
                }
                '.' => (),
                '(' => break,
                _ => unreachable!("Unexpected pattern in function name."),
            },
            None => break,
        }

        if let Some((_, ch)) = chars.peek() {
            if *ch == '.' {
                let _ = chars.next();
            } else if *ch == '(' {
                break;
            }
        }
    }
    Ok(components)
}

fn args_patterns(
    line: &str,
    chars: &mut Peekable<CharIndices>,
) -> Result<Vec<PracticeFuncArgumentPattern>, ()> {
    let Some((_, ch)) = chars.next() else {
        return Err(());
    };

    if ch != '(' {
        return Err(());
    }

    let mut args: Vec<PracticeFuncArgumentPattern> = Vec::new();
    let mut seg: Vec<PracticeFuncArgumentPattern> = Vec::new();

    let mut chained: bool = false;

    loop {
        if chained {
            assert_eq!(seg.len(), 1);

            let chain = chained_pattern(
                line,
                seg.pop().expect("There must be a prior value."),
                chars,
            );
            args.push(chain);
        }
        chained = false;

        match chars.peek() {
            Some((_, ch)) => match ch {
                '<' => {
                    let Some(pat) = angled_arg_pattern(line, chars) else {
                        return Err(());
                    };

                    if let Some((_, '.')) = chars.peek() {
                        chained = true;
                    }
                    seg.push(pat);
                }
                '{' => {
                    let Some(pat) = braced_pattern(line, chars) else {
                        return Err(());
                    };
                    seg.push(pat);
                }
                '"' => {
                    let Some(pat) = quoted_pattern(line, chars) else {
                        return Err(());
                    };
                    seg.push(pat);
                }
                '[' => {
                    let Some(pat) = optional_pattern(line, chars) else {
                        return Err(());
                    };
                    seg.push(pat);
                }
                '|' => {
                    assert_eq!(seg.len(), 1);
                    let Some(pat) = choice_pattern(line, seg.pop().unwrap(), chars) else {
                        return Err(());
                    };
                    assert!(seg.is_empty());

                    seg.push(pat);
                }
                ch if ch.is_alphanumeric() => {
                    let Some(pat) = identifier(line, chars) else {
                        return Err(());
                    };
                    seg.push(pat);
                }
                _ => (),
            },
            None => break,
        }

        // See `OS.FILE.BASENAME(<path>[,"<suffix>"])`
        if let Some((_, ch)) = chars.peek()
            && ['[', '|', '{'].contains(ch)
        {
            continue;
        }

        if let Some((_, ch)) = chars.next() {
            match ch {
                ',' => {
                    args.append(&mut seg);
                }
                ')' => {
                    args.append(&mut seg);
                    return Ok(args);
                }
                _ => (),
            }
        }
    }
    Ok(args)
}

fn angled_name_pattern(
    line: &str,
    chars: &mut Peekable<CharIndices>,
) -> Option<PracticeFuncNamePattern> {
    let Some((start, ch)) = chars.next() else {
        return None;
    };

    if ch != '<' {
        return None;
    }

    if let None = pattern_name(chars) {
        return None;
    }

    let Some((end, ch)) = chars.next() else {
        return None;
    };

    if ch != '>' {
        return None;
    }
    Some(PracticeFuncNamePattern::Angled(String::from(
        &line[(start + 1)..end],
    )))
}

fn angled_arg_pattern(
    line: &str,
    chars: &mut Peekable<CharIndices>,
) -> Option<PracticeFuncArgumentPattern> {
    let Some((start, ch)) = chars.next() else {
        return None;
    };

    if ch != '<' {
        return None;
    }

    if let None = pattern_name(chars) {
        return None;
    }

    let Some((end, ch)) = chars.next() else {
        return None;
    };

    if ch != '>' {
        return None;
    }
    Some(PracticeFuncArgumentPattern::Angled(String::from(
        &line[(start + 1)..end],
    )))
}

fn braced_pattern(
    line: &str,
    chars: &mut Peekable<CharIndices>,
) -> Option<PracticeFuncArgumentPattern> {
    let Some((start, ch)) = chars.next() else {
        return None;
    };

    if ch != '{' {
        return None;
    }

    if let Some((_, ',')) = chars.peek() {
        let _ = chars.next();
    }

    let pat = if let Some((_, '<')) = chars.peek() {
        if let Some(p) = angled_arg_pattern(line, chars) {
            p
        } else {
            return None;
        }
    } else if let Some(end) = pattern_name(chars) {
        PracticeFuncArgumentPattern::Literal(String::from(&line[(start + 1)..end]))
    } else {
        return None;
    };

    let Some((_, ch)) = chars.next() else {
        return None;
    };

    if ch != '}' {
        return None;
    }
    Some(PracticeFuncArgumentPattern::Braced(Box::new(pat)))
}

fn quoted_pattern(
    line: &str,
    chars: &mut Peekable<CharIndices>,
) -> Option<PracticeFuncArgumentPattern> {
    let Some((start, ch)) = chars.next() else {
        return None;
    };

    if ch != '"' {
        return None;
    }

    let Some(pat) = (match chars.peek() {
        Some((_, '<')) => angled_arg_pattern(line, chars),
        Some((_, '{')) => braced_pattern(line, chars),
        Some((end, '"')) => Some(PracticeFuncArgumentPattern::Literal(String::from(
            &line[(start + 1)..*end],
        ))),
        _ => None,
    }) else {
        return None;
    };

    let Some((_, ch)) = chars.next() else {
        return None;
    };

    if ch != '"' {
        return None;
    }
    Some(PracticeFuncArgumentPattern::Quoted(Box::new(pat)))
}

fn optional_pattern(
    line: &str,
    chars: &mut Peekable<CharIndices>,
) -> Option<PracticeFuncArgumentPattern> {
    let Some((_, ch)) = chars.next() else {
        return None;
    };

    if ch != '[' {
        return None;
    }

    let is_other: bool = if let Some((_, ch)) = chars.peek()
        && *ch == ','
    {
        let _ = chars.next().unwrap();
        true
    } else {
        false
    };

    let mut chained: bool = false;

    let mut pat: Option<PracticeFuncArgumentPattern> = None;
    let mut args: Vec<PracticeFuncArgumentPattern> = Vec::new();

    loop {
        if chained {
            assert!(pat.is_some());

            let chain = chained_pattern(
                line,
                pat.take().expect("There must be a prior value."),
                chars,
            );
            args.push(chain);
        }
        chained = false;

        match chars.peek() {
            Some((_, ch)) => match ch {
                '<' => {
                    let Some(arg) = angled_arg_pattern(line, chars) else {
                        return None;
                    };

                    if let Some((_, ':')) = chars.peek() {
                        pat = Some(arg);
                        chained = true;
                    } else {
                        args.push(arg);
                    }
                }
                '"' => {
                    let Some(arg) = quoted_pattern(line, chars) else {
                        return None;
                    };
                    args.push(arg);
                }
                '[' => {
                    let Some(arg) = optional_pattern(line, chars) else {
                        return None;
                    };
                    args.push(arg);
                }
                _ => (),
            },
            None => break,
        }

        if let Some((_, next)) = chars.peek() {
            // See `CONNECTION.GetDriverError([<first_line_nr>[,<last_line_nr>]])`
            if ['[', '<', '"', ':'].contains(next) {
                continue;
            } else if *next == ']' {
                break;
            }
        }
        chars.next();
    }

    let Some((_, ch)) = chars.next() else {
        return None;
    };

    if ch != ']' {
        return None;
    }

    if is_other {
        Some(PracticeFuncArgumentPattern::OptionalOtherArg(args))
    } else {
        Some(PracticeFuncArgumentPattern::Optional(args))
    }
}

fn choice_pattern(
    line: &str,
    first: PracticeFuncArgumentPattern,
    chars: &mut Peekable<CharIndices>,
) -> Option<PracticeFuncArgumentPattern> {
    let Some((_, ch)) = chars.next() else {
        return None;
    };

    let ellipsis: bool = if ch == '.'
        && let Some((_, '.')) = chars.peek()
    {
        true
    } else {
        false
    };

    if !ellipsis && ch != '|' {
        return None;
    }

    let mut args: Vec<PracticeFuncArgumentPattern> = vec![first];

    let mut pat: Option<PracticeFuncArgumentPattern> = None;
    let mut chained: bool = false;

    loop {
        if chained {
            assert!(pat.is_some());

            let chain = chained_pattern(
                line,
                pat.take().expect("There must be a prior value."),
                chars,
            );
            args.push(chain);
        }
        chained = false;

        match chars.peek() {
            Some((_, ch)) => match ch {
                '<' => {
                    let Some(arg) = angled_arg_pattern(line, chars) else {
                        return None;
                    };

                    if let Some((_, '.')) = chars.peek() {
                        pat = Some(arg);
                        chained = true;
                    } else {
                        args.push(arg);
                    }
                }
                '"' => {
                    let Some(pat) = quoted_pattern(line, chars) else {
                        return None;
                    };
                    args.push(pat);
                }
                '[' => {
                    if let Some(',') = peekn(line, chars, NonZeroU32::new(2).unwrap()) {
                        break;
                    }

                    let Some(arg) = optional_pattern(line, chars) else {
                        return None;
                    };

                    if let Some((_, '<')) = chars.peek() {
                        pat = Some(arg);
                        chained = true;
                    } else {
                        args.push(arg);
                    }
                }
                ch if ch.is_ascii_alphabetic() => {
                    let Some(pat) = identifier(line, chars) else {
                        return None;
                    };
                    args.push(pat);
                }
                _ => (),
            },
            None => break,
        }

        if let Some((_, next)) = chars.peek() {
            // See `InterCom.GetPracticeState(<intercom_name> | [<host>:]<port_number>)`
            if ['[', '<', '"'].contains(next) {
                continue;
            } else if [',', ')'].contains(next) {
                // See `InterCom.GetGlobalMacro(<name> | <host:port>,\"<macro name>\")`
                break;
            }
        }
        chars.next();
    }

    if ellipsis {
        Some(PracticeFuncArgumentPattern::Ellipsis(args))
    } else {
        Some(PracticeFuncArgumentPattern::Choice(args))
    }
}

fn chained_pattern(
    line: &str,
    first: PracticeFuncArgumentPattern,
    chars: &mut Peekable<CharIndices>,
) -> PracticeFuncArgumentPattern {
    let mut chain = vec![first];

    loop {
        match chars.peek() {
            Some((_, '<')) => {
                // See `InterCom.GetPracticeState(<intercom_name> | [<host>:]<port_number>)`
                'inner: loop {
                    match chars.peek() {
                        Some((_, '<')) => (),
                        _ => break 'inner,
                    }

                    let Some(child) = angled_arg_pattern(line, chars) else {
                        break 'inner;
                    };
                    chain.push(child);
                }
                assert!(chain.len() > 1);
                break PracticeFuncArgumentPattern::Chained(chain);
            }
            Some((_, '.')) => {
                // See `AVX512(<register_name>.<column_number>)`
                'inner: loop {
                    let Some(child) = angled_arg_pattern(line, chars) else {
                        break 'inner;
                    };
                    chain.push(child);

                    match chars.peek() {
                        Some((_, '.')) => (),
                        _ => break 'inner,
                    }
                }
                assert!(chain.len() > 1);
                break PracticeFuncArgumentPattern::Segmented(chain);
            }
            Some((_, ':')) => {
                chain.push(PracticeFuncArgumentPattern::Literal(":".to_string()));

                let _ = chars.next();
            }
            Some((_, ch)) if ch.is_alphabetic() => {
                // See `VPU(<register_name>.W0 .. .W3)"`
                let Some(arg) = identifier(line, chars) else {
                    unreachable!("Must resolve to keyword.")
                };

                while let Some((_, ' ')) = chars.peek() {
                    let _ = chars.next();
                }

                let Some(child) = choice_pattern(line, arg, chars) else {
                    unreachable!("Must resolve to ellipsis.")
                };
                chain.push(child);

                assert!(chain.len() > 1);
                break PracticeFuncArgumentPattern::Segmented(chain);
            }
            Some((_, ' ')) => {
                while let Some((_, ' ')) = chars.peek() {
                    let _ = chars.next();
                }
            }
            Some((_, ']')) => break PracticeFuncArgumentPattern::Chained(chain),
            _ => unreachable!("Other characters are not expected."),
        }
    }
}

fn identifier<'a>(
    line: &'a str,
    chars: &mut Peekable<CharIndices>,
) -> Option<PracticeFuncArgumentPattern> {
    let Some((start, _)) = chars.peek() else {
        return None;
    };
    let start = start.clone();

    let mut idx = start;

    let end = loop {
        if let Some((_, ch)) = chars.peek()
            && ch.is_alphanumeric()
        {
            let _ = chars.next();
            idx += 1;
            continue;
        }
        break idx;
    };
    Some(PracticeFuncArgumentPattern::Literal(String::from(
        &line[start..end],
    )))
}

fn pattern_name(chars: &mut Peekable<CharIndices>) -> Option<usize> {
    loop {
        match chars.peek() {
            Some((idx, ch)) => match ch {
                ch if ch.is_ascii_alphabetic() || ['_', ':', ' '].contains(&ch) => (),
                '.' | '(' | '>' => {
                    break Some(idx - 1);
                }
                _ => (),
            },
            None => break None,
        }
        let _ = chars.next();
    }
}

fn year(chars: &mut CharIndices) -> bool {
    for _ in 0..4 {
        let Some((_, ch)) = chars.next() else {
            return false;
        };

        if !ch.is_ascii_digit() {
            return false;
        }
    }
    true
}

fn word(chars: &mut CharIndices) -> Option<usize> {
    loop {
        match chars.next() {
            Some((idx, ch)) => {
                if ch.is_ascii_alphabetic() {
                    continue;
                } else if ch == ' ' {
                    break Some(idx);
                }
                break None;
            }
            None => break None,
        }
    }
}

fn peek_word(word: &str, substring: &str) -> bool {
    if word.len() <= 0 {
        return false;
    }

    let mut letter: usize = 0;
    for ch in substring.chars() {
        if ch != word.chars().nth(letter).unwrap() {
            return false;
        }

        if letter >= word.len() - 1 {
            return true;
        }
        letter += 1;
    }
    false
}

fn consumen(num: u32, chars: &mut Peekable<CharIndices>) {
    for _ in 0..num {
        let _ = chars.next();
    }
}

fn peekn(string: &str, chars: &mut Peekable<CharIndices>, offset: NonZeroU32) -> Option<char> {
    let Some((idx, _)) = chars.peek() else {
        return None;
    };

    let off: usize = *idx + (u32::from(offset) as usize) - 1;
    if off < string.len() {
        Some(string.as_bytes()[off] as char)
    } else {
        None
    }
}
