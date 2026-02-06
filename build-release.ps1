# Setup environment
& "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\Launch-VsDevShell.ps1" -Arch amd64

# Add tools to PATH
$env:Path = "C:\Strawberry\perl\bin;$env:USERPROFILE\.cargo\bin;C:\Program Files\CMake\bin;$env:Path"

# Use NMake to fix cmake detection
$env:CMAKE_GENERATOR = "NMake Makefiles"

# Build
cd C:\Users\ajkel\moonlight-web-stream\moonlight-web\web-server
cargo build --release
