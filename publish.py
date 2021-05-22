from __future__ import print_function

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
c("go build -o %s/%s.exe" % (deploy_target, prjname))
c(r"xcopy deploy_extra\* %s\\*" % deploy_target)
os.chdir("deploy")
c("zip -r %s %s" % (prjname, prjname) )