mkdir ..\out
copy subdir\testfile.txt ..\out
echo "making elsewhere"
md ..\elsewhere
touch ../elsewhere/singlefile.txt
echo "buildsomething done"