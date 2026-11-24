" Vim indent file for OmniScript (.omni) — supports both C/Rust braces and Python colons
if exists("b:did_indent")
  finish
endif
let b:did_indent = 1

setlocal indentexpr=GetOmniIndent(v:lnum)
setlocal indentkeys=0{,0},0),0],:,!^F,o,O,e

if exists("*GetOmniIndent")
  finish
endif

function! GetOmniIndent(lnum)
  let prevlnum = prevnonblank(a:lnum - 1)
  if prevlnum == 0
    return 0
  endif

  let prevline = getline(prevlnum)
  let curline  = getline(a:lnum)
  let ind      = indent(prevlnum)

  if prevline =~ '{\s*$\|:\s*$'
    let ind += shiftwidth()
  endif

  if curline =~ '^\s*}'
    let ind -= shiftwidth()
  endif

  return ind < 0 ? 0 : ind
endfunction
