# Interface languages

How a language of OpenRage's screens is made, changed and chosen, and the words the French catalog uses. How a developer writes a new sentence in the code, and what the gates check, is in [Crate_Reference.md](../reference/Crate_Reference.md#interface-languages).

## What there is

English is the text in the code and has no file. Nine languages are built in: German, Spanish, French, Italian, Japanese, Korean, Portuguese as Brazil writes it, Russian and Simplified Chinese. Any other language is a file a person adds. The gates hold all nine complete; in a language a person adds, a sentence that has no entry yet is shown in English. Settings, Appearance, Language lists them, each in its own name (`Français`), beside "System default", which is the language of the operating system when there is a file for it and English when there is not. The choice is kept with the preferences and applies at once, with no restart; a language the preferences name and no file has any more is ignored, and the system's is used.

## Making or changing a language

1. Copy [fr.po](../../src/i18n/languages/fr.po) to `CODE.po`, with the short code of the language (`de`, `pt`) as its name, into the `languages` folder inside the configuration directory (the folder the preferences file is in, `openrage` under the user's configuration folder, or the one `OPENRAGE_CONFIG_DIR` names). A file with the code of a built-in language is laid over it: an entry it has replaces the built-in one, and every other keeps its built-in text.
2. Set the header, the first entry, which has no `msgid`: `Language-Name` is the language in its own words, and `Plural-Forms` is `nplurals=2; plural=(n != 1);` when one is the only number that takes the first form, or `nplurals=2; plural=(n > 1);` when nought takes it too, as French does. Russian's three forms are `nplurals=3; plural=(n%10==1 && n%100!=11 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);`, as [ru.po](../../src/i18n/languages/ru.po) writes it. A language that has one form for every number, as Chinese, Japanese and Korean have, takes the first rule and writes its form twice. Another rule is not understood yet; the log says so and `n != 1` is used.
3. Write each `msgstr` under its English `msgid`. A plural entry has `msgid_plural` and two forms, `msgstr[0]` for the number the first form is for and `msgstr[1]` for the rest; under Russian's rule it has three, and `msgstr[2]` is for the rest. A form may take the names of either English form, so a first form that also serves 21 says `{count}` where the English says "1". Keep every `{name}` of the English text, spelled as it is: a translation that drops or renames one is wrong, and what the name stands for is put in at that place. `{{` and `}}` are the braces themselves. In a new language, an empty `msgstr` or no entry shows the English.
4. Start the editor again. A sentence the file does not have is shown in English, and the log names it once.

Keep the file in UTF-8, and one `msgid` once. Entries are in the order of their English text, so a change to one is one change in a diff.

## Fonts

The fonts OpenRage carries draw Latin and Cyrillic letters. Chinese, Japanese and Korean are drawn with a font of the system: Microsoft YaHei, Yu Gothic and Malgun Gothic on Windows, and Noto Sans CJK or Arial Unicode where a Linux or macOS machine has one. The three languages share characters that each country draws in its own shapes, so the font of the language in use is asked first. A machine with none of these fonts draws boxes.

Arabic, Hebrew, Hindi and the other scripts whose letters join or run from right to left cannot be drawn yet, so a file for one of them is of no use.

## French

The words the whole French catalog follows:

| English | French | Why |
|---|---|---|
| Content | Contenu | what a project holds |
| Builds | Builds | the word is the same in French |
| Open | Ouvrir | looks at a file where it is and changes nothing |
| Import | Importer | makes a game file a source of the project; the game file is never changed |
| Legacy, Enhanced | Legacy, Enhanced | the names of the two editions, which are never translated |
| Wardrobe | Garde-robe | the workspace of the clothing |
| Assets | Éléments | what the table of a project lists |

Sentences are plain and complete: no sentence is a noun and a verb put side by side to save room. Where a control is too narrow for the French, the French is shortened (`Ajouter…` for the Wardrobe's button, whose hover says what it adds) and the English is left as it is. The typography is French: a no-break space stands before `:`, `;`, `?` and `!`, and inside « and », so a line never breaks between a word and its mark.

## What is not translated

The renderer, the particle and animation editors and the developer tools are English, as are the sentences a library crate writes (the rules of the Wardrobe, of the weapons and of the particles, a build's report), the names of the engine's types and the values a person typed. A language file can still hold an entry for any sentence that is asked for through the mechanism, so a sentence moves into a language the day its code is wrapped.
