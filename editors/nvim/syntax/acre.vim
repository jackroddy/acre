" acre drawings: braced nodes of `key values;` properties
if exists("b:current_syntax")
  finish
endif

syn match acreKey /\%(^\|[{;]\)\s*\zs[A-Za-z_][A-Za-z0-9_-]*/
syn match acreNumber /-\?\<\d\+\%(\.\d*\)\?\>/
syn match acreColor /#\x\{6}\%(\x\x\)\?\>/
syn match acrePunct /[{};()+*\/]/
syn match acreComment /\/\/.*$/ contains=@Spell

syn region acreString start=/"/ skip=/\\./ end=/"/ contains=acreEscape,acreMarkup
syn match acreEscape /\\./ contained
syn match acreMarkup /`\|\*\*\|{[^ "]*\|}/ contained

hi def link acreKey Statement
hi def link acreNumber Number
hi def link acreColor Constant
hi def link acrePunct Delimiter
hi def link acreComment Comment
hi def link acreString String
hi def link acreEscape SpecialChar
hi def link acreMarkup Special

let b:current_syntax = "acre"
