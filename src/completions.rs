//! Shell completion scripts, written by hand.

const BASH: &str = r#"_huffman() {
    local cur=${COMP_WORDS[COMP_CWORD]}
    if [ "$COMP_CWORD" -eq 1 ]; then
        COMPREPLY=($(compgen -W "encode decode inspect completions help --help --version" -- "$cur"))
        return
    fi
    case ${COMP_WORDS[1]} in
        completions) COMPREPLY=($(compgen -W "bash zsh fish" -- "$cur")) ;;
        help) COMPREPLY=($(compgen -W "encode decode inspect completions" -- "$cur")) ;;
        encode | decode)
            if [[ $cur == -* ]]; then
                COMPREPLY=($(compgen -W "--verbose --help" -- "$cur"))
            else
                COMPREPLY=($(compgen -f -- "$cur"))
            fi
            ;;
        inspect) COMPREPLY=($(compgen -f -- "$cur")) ;;
    esac
}
complete -o filenames -F _huffman huffman
"#;

const ZSH: &str = r#"#compdef huffman
_huffman() {
    local -a commands
    commands=(
        'encode:compress a file'
        'decode:restore a file written by encode, checking it'
        'inspect:show the code table, tree and summary for a plain file'
        'completions:print a shell completion script'
        'help:show help for a command'
    )
    if (( CURRENT == 2 )); then
        _describe 'command' commands
        return
    fi
    case $words[2] in
        completions) _values 'shell' bash zsh fish ;;
        help) _describe 'command' commands ;;
        encode | decode) _arguments '(-v --verbose)'{-v,--verbose}'[print sizes when done]' '*:file:_files' ;;
        inspect) _files ;;
    esac
}
if [ "$funcstack[1]" = "_huffman" ]; then
    _huffman "$@"
else
    compdef _huffman huffman
fi
"#;

const FISH: &str = r#"complete -c huffman -f -n __fish_use_subcommand -a encode -d 'Compress a file'
complete -c huffman -f -n __fish_use_subcommand -a decode -d 'Restore a file written by encode, checking it'
complete -c huffman -f -n __fish_use_subcommand -a inspect -d 'Show the code table, tree and summary'
complete -c huffman -f -n __fish_use_subcommand -a completions -d 'Print a shell completion script'
complete -c huffman -f -n __fish_use_subcommand -a help -d 'Show help for a command'
complete -c huffman -n __fish_use_subcommand -s h -l help -d 'Show help'
complete -c huffman -n __fish_use_subcommand -s V -l version -d 'Show the version'
complete -c huffman -F -n '__fish_seen_subcommand_from encode decode inspect'
complete -c huffman -n '__fish_seen_subcommand_from encode decode' -s v -l verbose -d 'Print sizes when done'
complete -c huffman -f -n '__fish_seen_subcommand_from completions' -a 'bash zsh fish'
complete -c huffman -f -n '__fish_seen_subcommand_from help' -a 'encode decode inspect completions'
"#;

/// The completion script for `shell`, if it is one of bash, zsh or fish.
pub fn script(shell: &str) -> Option<&'static str> {
    match shell {
        "bash" => Some(BASH),
        "zsh" => Some(ZSH),
        "fish" => Some(FISH),
        _ => None,
    }
}
