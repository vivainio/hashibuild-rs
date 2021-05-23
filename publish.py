import os,shutil

def c(s):
    print(">",s)
    err = os.system(s)
    assert not err

def nuke(pth):
    if os.path.isdir(pth):
        shutil.rmtree(pth)

prjname = "hashibuild"
nuke("deploy")
deploy_target = r"deploy\%s" % prjname
c("cargo build --release --target-dir %s" % deploy_target)
# shutil.copy("target/release/hashibuild.exe", deploy_target + "/hashibuild.exe")
c(r"xcopy deploy_extra\* %s\\*" % deploy_target)

os.chdir("deploy")
c("zip -r %s %s" % (prjname, prjname) )