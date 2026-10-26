# Caret shell integration for fish — OSC 133 / OSC 7 / private OSC 7770;cmd;<base64>.
if status is-interactive; and not set -q CARET_SHELL_INTEGRATION_LOADED
    set -g CARET_SHELL_INTEGRATION_LOADED 1
    function __caret_osc
        printf '\e]%s\a' $argv[1]
    end
    function __caret_preexec --on-event fish_preexec
        set -g __caret_in_cmd 1
        __caret_osc "7770;cmd;"(printf '%s' "$argv" | base64 | tr -d '\n')
        __caret_osc "133;B"
        __caret_osc "133;C"
    end
    function __caret_postexec --on-event fish_postexec
        __caret_osc "133;D;$status"
        set -e __caret_in_cmd
    end
    function __caret_prompt --on-event fish_prompt
        __caret_osc "7;file://$hostname$PWD"
        __caret_osc "133;A"
    end
end
