" Pulpo's look for vim and nvim when they have no config of their own: the
" Linear Dark colors of the viewer (see theme.rs), and a few sane defaults.
" In a terminal vim takes bold, underline… from cterm=, so each group sets both.

set nocompatible backspace=indent,eol,start
set termguicolors background=dark
set number cursorline mouse=a laststatus=2 ruler showcmd incsearch hlsearch
filetype plugin indent on
syntax on

hi clear
let g:colors_name = 'pulpo'

" Editor
hi Normal       guifg=#E3E4E7 guibg=#101012 cterm=NONE
hi LineNr       guifg=#595A5C guibg=#101012 cterm=NONE
hi CursorLineNr guifg=#E3E4E7 guibg=#16171A gui=bold cterm=bold
hi CursorLine   guibg=#16171A gui=NONE cterm=NONE
hi SignColumn   guibg=#101012 cterm=NONE
hi Visual       guibg=#272B4B cterm=NONE
hi Search       guifg=NONE guibg=#5E4C37 cterm=NONE
hi IncSearch    guifg=#101012 guibg=#FCC581 gui=NONE cterm=NONE
hi CurSearch    guifg=#101012 guibg=#FCC581 cterm=NONE
hi MatchParen   guifg=#FFFFFF guibg=#272B4B gui=bold cterm=bold
hi NonText      guifg=#232429 cterm=NONE
hi EndOfBuffer  guifg=#232429 cterm=NONE
hi SpecialKey   guifg=#595A5C cterm=NONE
hi Folded       guifg=#97979A guibg=#16171A cterm=NONE
hi VertSplit    guifg=#232429 guibg=#101012 gui=NONE cterm=NONE
hi WinSeparator guifg=#232429 guibg=#101012 cterm=NONE
hi StatusLine   guifg=#E3E4E7 guibg=#1E2025 gui=NONE cterm=NONE
hi StatusLineNC guifg=#97979A guibg=#16171A gui=NONE cterm=NONE
hi TabLine      guifg=#97979A guibg=#16171A gui=NONE cterm=NONE
hi TabLineSel   guifg=#FFFFFF guibg=#5E6AD2 gui=bold cterm=bold
hi TabLineFill  guibg=#090909 gui=NONE cterm=NONE
hi Pmenu        guifg=#E3E4E7 guibg=#16171A cterm=NONE
hi PmenuSel     guifg=#FFFFFF guibg=#5E6AD2 cterm=NONE
hi PmenuSbar    guibg=#1E2025 cterm=NONE
hi PmenuThumb   guibg=#595A5C cterm=NONE
hi WildMenu     guifg=#FFFFFF guibg=#5E6AD2 cterm=NONE
hi ModeMsg      guifg=#97979A gui=bold cterm=bold
hi MoreMsg      guifg=#4CB782 cterm=NONE
hi Question     guifg=#4CB782 cterm=NONE
hi ErrorMsg     guifg=#EB5757 guibg=NONE cterm=NONE
hi WarningMsg   guifg=#FCC581 cterm=NONE
hi Directory    guifg=#8FA4FF cterm=NONE
hi Title        guifg=#55CDFF gui=bold cterm=bold

" Code: the viewer's token colors
hi Comment      guifg=#585A5C gui=italic cterm=italic
hi Statement    guifg=#58EBDD gui=NONE cterm=NONE
hi Keyword      guifg=#58EBDD cterm=NONE
hi PreProc      guifg=#58EBDD cterm=NONE
hi StorageClass guifg=#58EBDD cterm=NONE
hi Structure    guifg=#58EBDD cterm=NONE
hi Typedef      guifg=#58EBDD cterm=NONE
hi Operator     guifg=#D4D4D4 cterm=NONE
hi Delimiter    guifg=#D4D4D4 cterm=NONE
hi Type         guifg=#55CDFF gui=NONE cterm=NONE
hi Constant     guifg=#94A8FF cterm=NONE
hi String       guifg=#F1ADE0 cterm=NONE
hi Character    guifg=#F1ADE0 cterm=NONE
hi SpecialChar  guifg=#58EBDD cterm=NONE
hi Special      guifg=#58EBDD cterm=NONE
hi Function     guifg=#FCC581 cterm=NONE
hi Identifier   guifg=#8FA4FF gui=NONE cterm=NONE
hi Tag          guifg=#58EBDD cterm=NONE
hi Underlined   guifg=#8FA4FF gui=underline cterm=underline
hi Todo         guifg=#FCC581 guibg=NONE gui=bold cterm=bold
hi Error        guifg=#EB5757 guibg=NONE cterm=NONE
hi SpellBad     gui=undercurl guisp=#EB5757 cterm=undercurl

" Diffs and merge conflicts
hi DiffAdd      guibg=#182822 cterm=NONE
hi DiffDelete   guifg=#EB5757 guibg=#301A1C cterm=NONE
hi DiffChange   guibg=#16171A cterm=NONE
hi DiffText     guibg=#272B4B cterm=NONE
hi Added        guifg=#4CB782 cterm=NONE
hi Removed      guifg=#EB5757 cterm=NONE
hi Changed      guifg=#8FA4FF cterm=NONE
hi link diffAdded   Added
hi link diffRemoved Removed

" This runs after the file is loaded: give it its syntax now.
filetype detect
