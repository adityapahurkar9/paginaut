; Paginaut's installer hooks. The app itself offers Paginaut for its file types in Windows (Open with, Default
; apps) so the installer and Settings › Default app share one list; it never makes itself the default.
; After installing: the types already offered keep their choice (paths refreshed), a first install offers
; PDF, XPS, e-books and comics. Before a real uninstall (not an update's): all withdrawn.

!macro NSIS_HOOK_POSTINSTALL
  ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --register-types'
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --unregister-types'
  ${EndIf}
!macroend
