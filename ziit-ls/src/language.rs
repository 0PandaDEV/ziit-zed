use std::path::Path;

pub fn detect_language(file_path: Option<&str>) -> Option<String> {
    let path = file_path?;
    let path = Path::new(path);
    let extension = path.extension()?.to_str()?;

    let language = match extension.to_lowercase().as_str() {
        "js" => "JavaScript",
        "jsx" => "JSX",
        "ts" => "TypeScript",
        "tsx" => "TSX",
        "html" | "htm" => "HTML",
        "css" => "CSS",
        "scss" | "sass" => "SCSS",
        "less" => "LESS",
        "vue" => "Vue.js",
        "svelte" => "Svelte",
        "astro" => "Astro",
        "rs" => "Rust",
        "c" => "C",
        "cpp" | "cc" | "cxx" | "c++" => "C++",
        "h" | "hpp" | "hxx" => "C++",
        "go" => "Go",
        "zig" => "Zig",
        "v" => "V",
        "java" => "Java",
        "kt" | "kts" => "Kotlin",
        "scala" | "sc" => "Scala",
        "groovy" | "gvy" => "Groovy",
        "clj" | "cljs" | "cljc" => "Clojure",
        "cs" => "CSharp",
        "fs" | "fsx" => "FSharp",
        "vb" => "Visual Basic",
        "py" | "pyw" | "pyi" => "Python",
        "rb" | "rbw" => "Ruby",
        "php" => "PHP",
        "pl" | "pm" => "Perl",
        "lua" => "Lua",
        "sh" | "bash" | "zsh" => "Shell Script",
        "fish" => "Fish",
        "ps1" | "psm1" | "psd1" => "PowerShell",
        "r" => "R",
        "hs" | "lhs" => "Haskell",
        "ml" | "mli" => "OCaml",
        "elm" => "Elm",
        "ex" | "exs" => "Elixir",
        "erl" | "hrl" => "Erlang",
        "purs" => "PureScript",
        "roc" => "Roc",
        "gleam" => "Gleam",
        "json" => "JSON",
        "jsonc" => "JSONC",
        "yml"
            if path
                .file_name()
                .and_then(|n| n.to_str())
                .map_or(false, |n| n.starts_with("docker-compose")) =>
        {
            "Docker Compose"
        }
        "yaml" | "yml" => "YAML",
        "toml" => "TOML",
        "xml" => "XML",
        "csv" => "CSV",
        "ini" | "cfg" => "ini",
        "env" => "env",
        "md" | "markdown" => "Markdown",
        "rst" => "reST",
        "tex" => "LaTeX",
        "adoc" | "asciidoc" => "AsciiDoc",
        "org" => "Org",
        "sql" => "SQL",
        "graphql" | "gql" => "GraphQL",
        "cypher" | "cyp" => "Cypher",
        "swift" => "Swift",
        "m" => "Objective-C",
        "dart" => "Dart",
        "tf" | "tfvars" => "Terraform",
        "hcl" => "HCL",
        "dockerfile" => "Dockerfile",
        "pp" => "Puppet",
        "proto" => "Proto",
        "wasm" | "wat" => "WebAssembly Text Format",
        "wgsl" => "Wgsl",
        "glsl" | "vert" | "frag" => "GLSL",
        "hlsl" => "HLSL",
        "sol" => "Solidity",
        "cairo" => "Cairo",
        "move" => "Move",
        "noir" => "Noir",
        "fe" => "Fe",
        "aiken" => "Aiken",
        "el" => "Elisp",
        "lisp" | "lsp" => "Lisp",
        "scm" | "ss" => "Scheme",
        "rkt" => "Racket",
        "jl" => "Julia",
        "d" => "D",
        "nim" => "Nim",
        "cr" => "Crystal",
        "pony" => "Pony",
        "ada" | "adb" | "ads" => "Ada",
        "pas" => "Pascal",
        "f90" | "f95" | "f03" | "f" | "for" => "Fortran",
        "cob" | "cbl" => "COBOL",
        "asm" | "s" => "Assembly",
        "bf" => "Brainfuck",
        "pkl" => "Pkl",
        "prisma" => "Prisma",
        "gd" => "GDScript",
        "gdshader" => "Godot Shader",
        "wren" => "Wren",
        "awk" => "AWK",
        "sed" => "sed",
        "jq" => "jq",
        "just" => "Just",
        "make" => "Make",
        "cmake" => "CMake",
        "ninja" => "Ninja",
        "bazel" | "bzl" => "Starlark",
        "nix" => "Nix",
        "dhall" => "Dhall",
        "jsonnet" => "Jsonnet",
        "cue" => "CUE",
        "kdl" => "Kdl",
        "ron" => "RON",

        _ => return None,
    };

    Some(language.to_string())
}

pub fn extract_file_name(file_path: Option<&str>) -> Option<String> {
    let path = file_path?;
    let path = Path::new(path);
    path.file_name()?.to_str().map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_language() {
        assert_eq!(detect_language(Some("test.rs")), Some("Rust".to_string()));
        assert_eq!(
            detect_language(Some("test.js")),
            Some("JavaScript".to_string())
        );
        assert_eq!(detect_language(Some("test.py")), Some("Python".to_string()));
        assert_eq!(
            detect_language(Some("/path/to/file.go")),
            Some("Go".to_string())
        );
        assert_eq!(detect_language(Some("unknown.xyz")), None);
    }

    #[test]
    fn test_extract_file_name() {
        assert_eq!(
            extract_file_name(Some("/path/to/file.rs")),
            Some("file.rs".to_string())
        );
        assert_eq!(
            extract_file_name(Some("file.rs")),
            Some("file.rs".to_string())
        );
        assert_eq!(extract_file_name(None), None);
    }
}
