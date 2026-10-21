; Abigail Preview installs beside stable Abigail. Its own product name,
; executable, and registry keys keep the shared payload guards on preview files.
; Like the stable lane, the preview installer never deletes family data.
!define ABIGAIL_PRODUCT_NAME "Abigail Preview"
!define ABIGAIL_MAIN_EXECUTABLE "Abigail Preview.exe"
!include "${__FILEDIR__}/nsis-mvp-hooks.nsh"
