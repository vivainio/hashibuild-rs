import os,shutil
import subprocess

def run(arg):
    cmd = '.\hashibuild.exe ' + arg
    print ">", cmd
    p = os.popen(cmd)
    out = p.read()
    status = p.close()
    print out
    if status:
        raise RuntimeError("Command failed with %d" % status)

    return out

subprocess.check_call(["go", "build"])
os.system('go build')

cfg = "--salt 3 --config test/testprj.json "
run(cfg)
manifest = run(cfg + "--manifest")

for part in ["buildsomething.cmd", "subdir/testfile.txt"]:
    assert part in manifest

assert "ignored.txt" not in manifest

def nuke(pth):
    if os.path.isdir(pth):
        shutil.rmtree(pth)

outdir = "test/out"
nuke(outdir)

buildcmd = cfg + '--build'
run(buildcmd)
assert os.path.exists("test/out/testfile.txt")

archivedir = os.path.abspath("test/tmp")
os.environ["HASHIBUILD_ARCHIVE"] = archivedir
# this should usully move the file to batch upload place, not do slow upload on build
os.environ["HASHIBUILD_UPLOADER"] = "echo [ZIP]"
nuke(archivedir)
nuke(outdir)
withzipping = run(buildcmd)
assert "Zipping" in withzipping
run(buildcmd)
arccont = os.listdir(archivedir)

assert len(arccont) == 1 and "hashibuildtest" in arccont[0]
zipcont = os.popen("7za l %s/%s" % (archivedir, arccont[0])).read()
assert "testfile.txt" in zipcont
run("--vacuum")
nuke(archivedir)

# test remote fetching

os.environ["HASHIBUILD_ARCHIVE_REMOTE"] = "https://github.com/vivainio/hashibuild/raw/master/test/fakeremote/[ZIP]"
run(buildcmd)

