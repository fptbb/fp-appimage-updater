use crate::cli::{COMMAND_DEFS, GLOBAL_OPTS};

pub fn run(shell: &str) -> anyhow::Result<()> {
    let cmds_str = COMMAND_DEFS
        .iter()
        .map(|c| c.name)
        .collect::<Vec<_>>()
        .join(" ");
    let global_opts_str = GLOBAL_OPTS.join(" ");

    let script = match shell {
        "bash" => {
            let mut cases = String::new();
            for cmd in COMMAND_DEFS {
                let opts_joined = cmd.opts.join(" ");
                cases.push_str(&format!(
                    "        {})\n            opts=\"{opts_joined}\"\n            ;;\n",
                    cmd.name
                ));
            }
            format!(
                r#"
_fp_appimage_updater() {{
    local cur prev cmds opts global_opts subcommand
    COMPREPLY=()
    cur="${{COMP_WORDS[COMP_CWORD]}}"
    prev="${{COMP_WORDS[COMP_CWORD-1]}}"
    
    cmds="{cmds_str}"
    global_opts="{global_opts_str}"

    if [[ ${{COMP_CWORD}} -eq 1 ]]; then
        if [[ ${{cur}} == -* ]]; then
            COMPREPLY=( $(compgen -W "${{global_opts}}" -- "${{cur}}") )
        else
            COMPREPLY=( $(compgen -W "${{cmds}} ${{global_opts}}" -- "${{cur}}") )
        fi
        return 0
    fi

    subcommand="${{COMP_WORDS[1]}}"
    case "${{subcommand}}" in
{cases}        *)
            opts=""
            ;;
    esac

    if [[ ${{cur}} == -* ]]; then
        COMPREPLY=( $(compgen -W "${{opts}} ${{global_opts}}" -- "${{cur}}") )
    else
        COMPREPLY=( $(compgen -W "${{opts}}" -- "${{cur}}") )
    fi
}}
complete -F _fp_appimage_updater fp-appimage-updater
"#
            )
        }
        "zsh" => {
            let mut cases = String::new();
            for cmd in COMMAND_DEFS {
                let opts_joined = cmd.opts.join(" ");
                cases.push_str(&format!(
                    "                {}) _arguments '*: :({opts_joined})' ;;\n",
                    cmd.name
                ));
            }
            format!(
                r#"
#compdef fp-appimage-updater
_fp_appimage_updater() {{
    local curcontext="$curcontext" state line
    typeset -A opt_args

    _arguments -C \
        '1: :({cmds_str})' \
        '*: :->args'

    case $state in
        args)
            case $line[1] in
{cases}            esac
            ;;
    esac
}}
if [[ "$funcstack[1]" == "_fp_appimage_updater" ]]; then
    _fp_appimage_updater "$@"
else
    compdef _fp_appimage_updater fp-appimage-updater
fi
"#
            )
        }
        "fish" => {
            let mut s = String::from("complete -c fp-appimage-updater -f\n");
            for opt in GLOBAL_OPTS {
                if let Some(stripped) = opt.strip_prefix("--") {
                    s.push_str(&format!("complete -c fp-appimage-updater -n 'not __fish_seen_subcommand_from {cmds_str}' -l '{stripped}'\n"));
                } else if let Some(stripped) = opt.strip_prefix('-') {
                    s.push_str(&format!("complete -c fp-appimage-updater -n 'not __fish_seen_subcommand_from {cmds_str}' -o '{stripped}'\n"));
                }
            }
            for cmd in COMMAND_DEFS {
                s.push_str(&format!("complete -c fp-appimage-updater -n 'not __fish_seen_subcommand_from {cmds_str}' -a '{}'\n", cmd.name));
                for opt in cmd.opts {
                    if let Some(stripped) = opt.strip_prefix("--") {
                        s.push_str(&format!("complete -c fp-appimage-updater -n '__fish_seen_subcommand_from {}' -l '{stripped}'\n", cmd.name));
                    } else if let Some(stripped) = opt.strip_prefix('-') {
                        s.push_str(&format!("complete -c fp-appimage-updater -n '__fish_seen_subcommand_from {}' -o '{stripped}'\n", cmd.name));
                    }
                }
            }
            s
        }
        _ => return Err(anyhow::anyhow!("Unsupported shell: {}", shell)),
    };

    println!("{}", script.trim());
    Ok(())
}
