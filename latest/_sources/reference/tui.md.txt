<!-- Generated from the TUI's key bindings by the test the_reference_page_is_written_from_the_bindings (tests/tui_model.rs):
     do not edit; change the bindings, then run
     SAMPLEKIT_WRITE_REFERENCE=1 cargo test --test tui_model the_reference_page -->

# The TUI's keys

Every key of the TUI, screen by screen, as `?` lists them there. The foot of each screen shows the keys used most.

In a list to choose from, `Space` ticks and `Enter` accepts what is ticked, and `/` narrows the list to what holds the letters typed. Every line typed has a cursor that `←` `→` `Home` `End` move; a sample's value opens selected, so that what is typed replaces it, and an arrow or `End` keeps it to edit.

## Typing

In every line typed (the filter, a prompt, a value's window, a list's `/`, the start page's name and narrowing) and in the note. A window's `Enter`, `Esc` and `Tab` stay the window's.

| Keys | What they do |
| --- | --- |
| `←` `→` `Home` `End` | move; Ctrl+← Ctrl+→ by word |
| `Shift+←` `Shift+→` `Shift+Home` `Shift+End` | select; Ctrl+Shift+← Ctrl+Shift+→ by word; Ctrl+A all |
| `Backspace` `Delete` | take a character out, or what is selected; what is typed replaces it |
| `Ctrl+Backspace` `Ctrl+Delete` | take a word out |
| `Ctrl+Z` | undo |
| `Ctrl+Shift+Z` `Alt+Z` | redo; Alt+Z where the terminal sends Ctrl+Shift+Z as Ctrl+Z |
| `Ctrl+X` `Ctrl+V` | cut, paste |
| `Ctrl+C` | copy what is selected; with nothing selected, close SampleKit, asked first over a line typed and not saved |
| `Ctrl+D` | empty the line; in the note, the line doubled |
| `click` | place the cursor; a drag selects, a double click a word |

**In the note** (`N` on a sample), besides:

| Keys | What they do |
| --- | --- |
| `*` `_` `~` | put around what is selected |
| `Alt+1…6` | a heading of that level; the same again, none |
| `Alt+L` | a link around what is selected |
| `Alt+I` | an image |
| `Alt+C` | a code block |
| `Alt+K` | a reference link |
| `Alt+R` | a link's definition |
| `Alt+F` | a footnote |
| `Enter` | a new line, indented as this one; in a table, a new row |
| `Tab` | under a list item's text, to a table's next cell, or four columns in |
| `Shift+Tab` | a table's previous cell, or the selection's indentation taken back |
| `↑` `↓` `PgUp` `PgDn` | move; Ctrl+Home Ctrl+End to its start, its end |
| `Ctrl+Y` | the line deleted |
| `Esc` | done: the change previewed |

## The start page

`samplekit` alone, in a terminal.

| Keys | What they do |
| --- | --- |
| `Enter` | open the project marked: the one SampleKit was started in comes first; started in a folder holding projects, those come first, then setting one up there |
| `1…9` | open that recent project |
| `x` | forget the recent project marked, asked first: its state removed, the folder kept |
| `j` `k` `↑` `↓` | move; Home End to the first and the last |
| `n` | New project: a folder moved to, or one + makes, then set up by a few questions; Esc on the first question comes back here, the folder + made removed if still empty |
| `o` | Open a folder: Enter on a project opens it, on any other goes in |
| `c` | Configure a project: a recent one holding a .samplekitrc, or another moved to |
| `g` | Guide and demo: the guide in the browser; the demo's page, its steps; the first time, the demo is written and its first step opened |
| `q` `Esc` | quit |
| `Ctrl+C` | quit, from the page or from a project |
| `?` | this help |

### Moving through folders

| Keys | What they do |
| --- | --- |
| `→` `l` | into the folder marked |
| `←` `h` | up; Backspace too |
| `~` | the home folder |
| `/` | narrow the folders by typing; Esc clears it, as going in does |
| `+` | a new folder here, for a new project |
| `Enter` | the folder itself, chosen; a project opened; another gone into |
| `Esc` | back to the page |

### The demo

| Keys | What they do |
| --- | --- |
| `1…5` | open that step; quitting it comes back to the demo's page |
| `Enter` | the step marked; the home page, which walks through the steps, in the browser; the demo's folder, in the file manager, to edit its files and try the command line there; or Reset the demo, asked first |
| `Esc` `q` | back to the start page |

A project opened from here comes back here when it is quit.

## The collection

What a project opens on: its samples, one per line. The basket, the samples `Space` puts in it, is kept from one session to the next, with the filter, the sort and the columns: `C`, `p`, `t` and `D` act on it alone while it holds any, and say so; `x` empties it.

**Moving**

| Keys | What they do |
| --- | --- |
| `j` `↓` | next sample |
| `k` `↑` | previous sample |
| `Home` | first sample |
| `End` | last sample |
| `PgDn` | a page down |
| `PgUp` | a page up |
| `Enter` `l` `→` | open the sample |
| `<` | the columns further left |
| `>` | the columns further right |

**Finding and showing**

| Keys | What they do |
| --- | --- |
| `f` | apply a saved query or a profile |
| `g` | group by fields, a heading above each group |
| `/` | filter, Tab completes |
| `Esc` | clear the filter |
| `s` | sort by a column |
| `c` | choose the columns |
| `b` | the basket only, or everything |
| `S` | the columns summarised, or the samples again |

**Choosing**

| Keys | What they do |
| --- | --- |
| `Space` | put in or take out of the basket, then the next row |
| `t` | tag the basket, take a tag off it, or rename a tag: old=new, over every sample shown where the basket is empty |
| `a` | put every sample shown in the basket |
| `x` | empty the basket |

**Changing**

| Keys | What they do |
| --- | --- |
| `N` | a new sample beside this one: its name, then field=value …, and keep:field,… from the one under the cursor |
| `u` | undo the last change written |
| `D` | remove the sample, or the basket's: its file, asked first, u gives it back |
| `R` | rename the sample, its own files and the links leading to it, asked first |

**Computing and checking**

| Keys | What they do |
| --- | --- |
| `C` | compute the selected values, or what is not current: the basket, or every sample shown |
| `v` | the collection's state: what is not current, what is defective |
| `H` | the project's history: each change kept, and what it changed |

**Figures and exports**

| Keys | What they do |
| --- | --- |
| `p` | run a script, draw a figure, or write an export: of the basket, or of what is shown; w on a figure writes it to a file |

**The project**

| Keys | What they do |
| --- | --- |
| `o` | the sample's own files |
| `E` | the sample in $EDITOR |
| `P` | the project's configuration, section by section |
| `M` | the project's model in $EDITOR |
| `W` | save what is shown in .samplekitrc: the filter, the columns, an export |

**The TUI**

| Keys | What they do |
| --- | --- |
| `?` | this help |
| `q` | quit |

## A sample

One sample, value by value, and its tables.

**Moving**

| Keys | What they do |
| --- | --- |
| `j` `↓` | next value |
| `k` `↑` | previous value |
| `Home` | first value |
| `End` | last value |
| `PgDn` | a page down |
| `PgUp` | a page up |
| `→` `l` | unfold a table |
| `Esc` `h` `←` `q` | back to the collection, or to the control screen it was opened from |
| `J` | scroll the note down; the wheel over it too |
| `K` | scroll the note up |

**Finding and showing**

| Keys | What they do |
| --- | --- |
| `n` | show or hide the note, remembered |

**Choosing**

| Keys | What they do |
| --- | --- |
| `Space` | select the value, for c |
| `a` | select every value a formula gives |
| `x` | clear the selection |

**Changing**

| Keys | What they do |
| --- | --- |
| `D` | remove this sample: its file, asked first, u gives it back |
| `R` | rename this sample, its own files and the links leading to it, asked first |
| `e` | change the value, previewed |
| `N` | edit the note |
| `A` | add an attribute: its name, then its value |
| `u` | undo the last change written |

**Computing and checking**

| Keys | What they do |
| --- | --- |
| `H` | this sample's history: each change kept, and what it changed |
| `Enter` | open the value in its window (value, uncertainty, readings), or an attribute's items; a link, its sample or its address |
| `c` | compute the selected values, or this one, even if current or edited |
| `C` | compute what is not current in the sample |

**Figures and exports**

| Keys | What they do |
| --- | --- |
| `p` | run a script, draw a figure, or write an export: of the basket, or of the sample |

**The project**

| Keys | What they do |
| --- | --- |
| `o` | the sample's own files, or where the link under the cursor leads |
| `E` | the sample in $EDITOR |

**The TUI**

| Keys | What they do |
| --- | --- |
| `?` | this help |

## A table

One table of a sample, row by row.

**Moving**

| Keys | What they do |
| --- | --- |
| `j` `↓` | next row |
| `k` `↑` | previous row |
| `Home` | first row |
| `End` | last row |
| `PgDn` | a page down |
| `PgUp` | a page up |
| `→` `l` | next column |
| `←` `h` | previous column; from the first, back to the sample |
| `Esc` `q` | back to the sample |

**Choosing**

| Keys | What they do |
| --- | --- |
| `Space` | select the column, for c |
| `x` | clear the selection |

**Changing**

| Keys | What they do |
| --- | --- |
| `e` | change the index or a text cell; on the empty row, add one; a value is changed in its window, Enter |
| `u` | undo the last change written |

**Computing and checking**

| Keys | What they do |
| --- | --- |
| `H` | the sample's history: each change kept, and what it changed |
| `Enter` | open the cell: its value, its uncertainty, where it came from; e there changes them |
| `c` | compute the selected columns, or this one, even if current or edited |
| `C` | compute what is not current in the sample |

**Figures and exports**

| Keys | What they do |
| --- | --- |
| `p` | a figure of this table: a column against another, the selected first |

**The project**

| Keys | What they do |
| --- | --- |
| `o` | the sample's own files |
| `E` | the sample in $EDITOR |

**The TUI**

| Keys | What they do |
| --- | --- |
| `?` | this help |

## A value's window

`Enter` on a value of a sample: its value, its uncertainty and its readings, a column each.

**Moving**

| Keys | What they do |
| --- | --- |
| `l` `→` | next column |
| `h` `←` | previous column; from the first, back to the sample |
| `Esc` `q` | back to the sample |

**Changing**

| Keys | What they do |
| --- | --- |
| `e` | change the cell, previewed; on readings, their list |
| `u` | undo the last change written |

**Computing and checking**

| Keys | What they do |
| --- | --- |
| `c` | compute the property, even if current or edited |
| `Enter` | where the value came from |

**The TUI**

| Keys | What they do |
| --- | --- |
| `?` | this help |

## A list's window

A value's readings, or an attribute's items, one per row.

**Moving**

| Keys | What they do |
| --- | --- |
| `j` `↓` | next item |
| `k` `↑` | previous item |
| `Esc` `q` `←` `h` | back |

**Changing**

| Keys | What they do |
| --- | --- |
| `e` | change the item, previewed; on the empty row, add one |
| `d` | take the item out, previewed |
| `J` | move the item down, previewed |
| `K` | move the item up, previewed |
| `L` | an attribute: a list of one to its single value, or back |
| `u` | undo the last change written |

**Computing and checking**

| Keys | What they do |
| --- | --- |
| `Enter` | where the item leads, where it is a link |

**The project**

| Keys | What they do |
| --- | --- |
| `o` | where the item leads, where it is a link: its sample, its file or its address |

**The TUI**

| Keys | What they do |
| --- | --- |
| `?` | this help |

## The collection's state

`v`: what is not current, what is defective.

**Moving**

| Keys | What they do |
| --- | --- |
| `j` `↓` | next |
| `k` `↑` | previous |
| `Home` | first |
| `End` | last |
| `PgDn` | a page down |
| `PgUp` | a page up |
| `Enter` | open the sample, on its value |
| `Esc` `q` | back to the collection |

**Changing**

| Keys | What they do |
| --- | --- |
| `u` | undo the last change written |

**Computing and checking**

| Keys | What they do |
| --- | --- |
| `C` | compute every sample with a value outdated, failed or never computed |

**The TUI**

| Keys | What they do |
| --- | --- |
| `?` | this help |

## The history

`H`: each change kept, and what it changed.

**Moving**

| Keys | What they do |
| --- | --- |
| `j` `↓` | next |
| `k` `↑` | previous |
| `Home` | the newest |
| `End` | the oldest |
| `PgDn` | a page down |
| `PgUp` | a page up |
| `J` | scroll what it changed down |
| `K` | scroll what it changed up |
| `Esc` `q` `←` `h` | back |

**The history**

| Keys | What they do |
| --- | --- |
| `r` | take the files back to this snapshot, previewed: restore --at |
| `d` | what this snapshot changed, and it marked; d on another compares the two: diff --from --to |

**The TUI**

| Keys | What they do |
| --- | --- |
| `?` | this help |

## Setting a project up

A folder with no project: a few questions.

**Moving**

| Keys | What they do |
| --- | --- |
| `j` `↓` | the next answer |
| `k` `↑` | the previous answer |
| `Enter` | this answer, then the next question; at the end, set the project up |
| `Esc` `q` | the previous question; from the first, back to the start page or the collection |

**The TUI**

| Keys | What they do |
| --- | --- |
| `?` | this help |

## The configuration

`P`: the project's `.samplekitrc`, section by section.

**Moving**

| Keys | What they do |
| --- | --- |
| `j` `↓` | next |
| `k` `↑` | previous |
| `→` `l` `Tab` | into the section |
| `←` `h` | back to the sections |
| `Enter` | change the setting; on an entry, add a key to it |
| `Esc` `q` | back to the collection, keeping the changes not written |

**Changing**

| Keys | What they do |
| --- | --- |
| `e` | change the setting; on an entry, add a key to it |
| `a` | add a setting: key = value, or name.key = value |
| `d` | remove the setting, or the whole entry |
| `w` | write the file, its changes previewed |
| `r` | read the file again, dropping the changes not written |

**The TUI**

| Keys | What they do |
| --- | --- |
| `?` | this help |

## Questions and lists

**A question before a write** shows what would change, as the command line's preview does:

| Keys | What they do |
| --- | --- |
| `y` | write it |
| `n` `Esc` | leave it: nothing is written |
| `j` `k` `↓` `↑` | read a long question to its end |

**A list to choose from**, as `s`, `c`, `g` or `p` open one:

| Keys | What they do |
| --- | --- |
| `j` `k` `↓` `↑` | move |
| `Home` `End` `PgUp` `PgDn` | first, last, a page |
| `Space` | in the sort, the columns and the groups, tick or untick |
| `Shift+↑` `Shift+↓` `K` `J` | in the sort, the columns and the groups, the item under the cursor up or down: its order |
| `r` | in the sort, the field under the cursor descending, or back, and ticked |
| `/` | narrow the list to what holds the letters typed |
| `Tab` | in `p`, the next group: scripts, figures, exports |
| `w` | in `p`, a figure to a file |
| `Enter` | accept |
| `Esc` | clear the narrowing, then leave the list |

## Marks

The marks beside a sample or a value, on the collection, a sample, a table and the collection's state.

| Mark | What it says |
| --- | --- |
| ● | in the basket |
| ⚠ | outdated: an input changed, or its record cannot be checked; C computes it |
| ✎ | edited: a value typed over its formula, or its record missing |
| ✗ | failed: its formula raised an error when last run |
| ∅ | never computed, or waits for an input: what the model owes |
| · | readings with no statistic chosen: nothing computes the value |
| ⊘ | defective, on the control screen: what validate refuses |
| ¶ | a note, on the control screen: what validate remarks on |
