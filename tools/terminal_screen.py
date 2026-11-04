"""What a terminal shows of a session captured with its escapes: each line,
and the style each of its characters was drawn in. A line rewritten in place,
a progress bar, keeps what was written last.

tools/check-docs.py reads the command line's colours through it, to hold the
documentation's to them.
"""

import re

COLOURS = {
    30: "black", 31: "red", 32: "green", 33: "yellow", 34: "blue", 35: "magenta",
    36: "cyan", 37: "grey", 90: "dark-grey", 91: "light-red", 92: "light-green",
    93: "light-yellow", 94: "light-blue", 95: "light-magenta", 96: "light-cyan",
    97: "white",
}
ESCAPE = re.compile(r"\x1b\[([0-9;]*)([A-Za-z])")


def screen(text):
    """[(line, [style of each character])], a style being `dim` (faint, as an
    identity column is drawn), `bold`, a colour's name, or `plain`. `text` is read as written: a carriage return goes back
    to the line's start."""
    lines, current, styles = [], [], []
    column, bold, dim, colour = 0, False, False, None
    at = 0
    while at < len(text):
        character = text[at]
        escape = ESCAPE.match(text, at) if character == "\x1b" else None
        if escape:
            parameters, command = escape.groups()
            if command == "m":
                for code in [int(part) for part in parameters.split(";") if part] or [0]:
                    if code == 0:
                        bold, dim, colour = False, False, None
                    elif code == 1:
                        bold = True
                    elif code == 2:
                        dim = True
                    elif code == 22:
                        bold, dim = False, False
                    elif code == 39:
                        colour = None
                    elif code in COLOURS:
                        colour = COLOURS[code]
            elif command == "K":
                if parameters == "2":
                    current, styles, column = [], [], 0
                else:
                    current, styles = current[:column], styles[:column]
            at = escape.end()
            continue
        if character == "\r":
            column = 0
        elif character == "\n":
            line = "".join(current).rstrip()
            lines.append((line, styles[: len(line)]))
            current, styles, column = [], [], 0
        else:
            style = "dim" if dim else colour or ("bold" if bold else "plain")
            if column < len(current):
                current[column], styles[column] = character, style
            else:
                current.append(character)
                styles.append(style)
            column += 1
        at += 1
    if current:
        line = "".join(current).rstrip()
        lines.append((line, styles[: len(line)]))
    return lines
