" Vim syntax file
" Language:    OmniScript (Polyglot Systems Language)
" Maintainer:  OmniScript Team
" Latest Revision: 2026

if exists("b:current_syntax")
  finish
endif

" Comments (`//`, `#` not followed by `[`, and `/* ... */`)
syn match  omniComment      "//.*$" contains=omniTodo
syn match  omniComment      "#\%(\[\)\@!.*$" contains=omniTodo
syn region omniBlockComment start="/\*" end="\*/" contains=omniTodo
syn keyword omniTodo        contained TODO FIXME XXX NOTE HACK

" Strings & escapes (including f/r/b-prefixed polyglot strings)
syn match  omniEscape       contained "\\[nrt\\\"'0e]\|\\x[0-9a-fA-F]\{2}"
syn region omniString       start='\<[fFrRbB]\?"' skip='\\"' end='"' contains=omniEscape
syn region omniString       start="\<[fFrRbB]\?'" skip="\\'" end="'" contains=omniEscape

" Decorators & Rust-style attributes
syn match  omniDecorator    "@[A-Za-z_][A-Za-z0-9_.]*"
syn region omniAttribute    start="#\[" end="\]"

" Polyglot Keywords
syn keyword omniKeyword     fn def func function let var const val mut pub public private static
syn keyword omniKeyword     extern declare struct class enum interface trait new
syn keyword omniConditional if else elif match case switch default
syn keyword omniRepeat      while for in loop break continue
syn keyword omniStatement   return yield defer go spawn async await with
syn keyword omniException   try catch except finally throw raise
syn keyword omniInclude     import using from as
syn keyword omniOperator    and or not is

" Builtin Types & Constants
syn keyword omniType        int i64 i32 i16 i8 u64 u32 u16 u8 usize isize
syn keyword omniType        float f64 f32 double str String bool boolean void char byte
syn keyword omniType        Vec Map List Dict Slice Chan Pointer Result Option
syn keyword omniBoolean     true false True False
syn keyword omniConstant    null None nil self this

" Numbers (hex, binary, float, int)
syn match  omniNumber       "\<0[xX][0-9a-fA-F_]\+\>"
syn match  omniNumber       "\<0[bB][01_]\+\>"
syn match  omniFloat        "\<[0-9][0-9_]*\.[0-9_]\+\%([eE][+-]\?[0-9_]\+\)\?\>"
syn match  omniNumber       "\<[0-9][0-9_]*\>"

" Function definitions & calls
syn match  omniFuncCall     "\<[A-Za-z_][A-Za-z0-9_]*\ze\s*("

" Operators
syn match  omniSymOperator  "->\|=>\|:=\|<-\|==\|!=\|<=\|>=\|&&\|||\|<<\|>>"

" Highlight links (works with every Vim/Neovim colorscheme)
hi def link omniComment      Comment
hi def link omniBlockComment Comment
hi def link omniTodo         Todo
hi def link omniString       String
hi def link omniEscape       SpecialChar
hi def link omniDecorator    PreProc
hi def link omniAttribute    PreProc
hi def link omniKeyword      Keyword
hi def link omniConditional  Conditional
hi def link omniRepeat       Repeat
hi def link omniStatement    Statement
hi def link omniException    Exception
hi def link omniInclude      Include
hi def link omniOperator     Operator
hi def link omniSymOperator  Operator
hi def link omniType         Type
hi def link omniBoolean      Boolean
hi def link omniConstant     Constant
hi def link omniNumber       Number
hi def link omniFloat        Float
hi def link omniFuncCall     Function

let b:current_syntax = "omni"
