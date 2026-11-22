import type {Terminal} from '@xterm/xterm';
import {UnicodeGraphemesAddon} from '@xterm/addon-unicode-graphemes';

/** Use the official grapheme implementation for combining marks and joined emoji. */
export function configureTerminalUnicode(term:Terminal):void {
  term.loadAddon(new UnicodeGraphemesAddon());
  term.unicode.activeVersion='15-graphemes';
}
