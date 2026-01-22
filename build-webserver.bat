@echo off
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
cd C:\Users\ajkel\moonlight-web-stream\moonlight-web\web-server
cargo build --release
