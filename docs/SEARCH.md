# File search

Gallery, Filmstrip and the file picker share an expression syntax based on
[Everything](https://www.voidtools.com/support/everything/search_syntax/).
Search filters the current surface; it does not add folders to the library or
change the filesystem. Gallery searches recorded visits, Filmstrip searches its
current folder, Ctrl+G searches immediate files and Ctrl+P searches recent files
and the current folder hierarchy.

| Expression | Meaning |
|---|---|
| `holiday beach` | Both terms |
| `holiday\|beach` | Either term |
| `!draft` | Exclude a term |
| `!<holiday beach>` | Exclude files matching both terms |
| `"summer holiday"` | Match that contiguous phrase, including the space |
| `*.mp4` | Match the whole filename with a wildcard |
| `photo-?.png` | Match one character with `?` |
| `ext:jpg;png` | Either extension, case insensitive |
| `image:`, `video:`, `audio:` | A media type supported by towavue |
| `folder:"C:/My Media"` | Files in that folder or its descendants |
| `parent:"C:/My Media"` | Files directly in that folder |
| `name:holiday`, `stem:holiday` | Search filename, or filename without its extension |
| `path:holiday` | Search the full path |
| `whole:"holiday.jpg"` | Match the entire filename |

OR has higher precedence than AND, following Everything's default:
`holiday image:|video:` means `holiday <image:|video:>`.
Use angle brackets to specify another grouping. Function groups and lists work
as well: `ext:<jpg|png>` and `ext:jpg;png` are equivalent. Quotes protect spaces
and boolean operators; wildcards remain available in quoted filename patterns.
`*` and `?` do not cross path separators; `**` can cross them.

Matching is case insensitive and accepts either path separator. Plain terms
search names in Filmstrip and the file picker; including a slash or drive prefix
searches paths. Gallery retains its existing full-path matching. Wildcards match
whole filenames unless the pattern includes a path or uses `path:`. The file
picker retains fuzzy ranking for a single unquoted plain term; expressions and
quoted phrases use strict matching. Gallery and Filmstrip retain their display
order instead of ranking results.

`folder:` is a towavue adaptation: it filters files inside a folder, whereas
Everything uses that name to select folder items. A full folder path respects
directory boundaries (`C:/Media` does not include `C:/Media2`). A folder-name
fragment searches parent paths. No filesystem traversal is added to Gallery or
Filmstrip by this function.

## Gallery dates

`dg:` (also `date-gallery:`) searches the local calendar date of the recorded
visit, the same date used by Gallery's date rail. It is available only in Gallery.

| Expression | Meaning |
|---|---|
| `dg:today`, `dg:yesterday` | A local calendar day |
| `dg:thisweek`, `dg:lastweek` | A calendar week, starting on Monday |
| `dg:thismonth`, `dg:lastmonth` | A calendar month |
| `dg:thisyear`, `dg:lastyear` | A calendar year |
| `dg:2026`, `dg:2026-10`, `dg:2026/10/23` | A year, month or day |
| `dg:2026/01/01..2026/10/23` | An inclusive range |
| `dg:>=2026-10`, `dg:<2026-10` | On/after, or before, a calendar boundary |
| `dg:2026-10..`, `dg:..2026-09` | An open-ended range |
| `dg:unknown` | Legacy visits without a recorded date |

`=`, `==`, `!`, `!=`, `<`, `<=`, `>` and `>=` are supported before date values.
For a month/year, equality covers that whole period and `>` starts after it.
Unknown dates do not match ordinary date comparisons. A leading NOT can include
unknown visits, for example `!dg:today`.

Size (`size:`), modification date (`dm:`) and creation date (`dc:`) are not
available: these properties are not retained in the browsing lists. Searches
never fetch metadata synchronously while drawing. Content search, regular
expressions, custom macros and other Everything functions are not implemented.
An invalid or unfinished expression has no results; Gallery and the file picker
use their existing empty-result message to explain malformed syntax. The search
field itself keeps its ordinary appearance.
