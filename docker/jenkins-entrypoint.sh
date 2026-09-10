#!/usr/bin/env bash
set -euo pipefail
umask 077
export HOME=/state/home
export TMPDIR=/state/tmp
mkdir -p /state/jenkins "$HOME" "$TMPDIR" /cache /target /releases /run/bootstrap
chown 1000:1000 /state/jenkins "$HOME" "$TMPDIR" /cache /target /releases /run/bootstrap
python3 /opt/mainframe-env/docker/storage.py check /state /cache /target /releases
if [[ ! -f "$JENKINS_HOME/controller/jenkins.war" ]]; then
  cp -a /opt/jenkins-seed/. "$JENKINS_HOME/"
  chown -R 1000:1000 "$JENKINS_HOME"
fi
python3 /opt/mainframe-env/tools/supply_chain.py verify-jenkins --home "$JENKINS_HOME"
mkdir -p "$JENKINS_HOME/init.groovy.d"
cp /opt/mainframe-env/docker/init.groovy "$JENKINS_HOME/init.groovy.d/10-mainframe-env.groovy"
cp /run/secrets/jenkins_password /run/bootstrap/jenkins_password
cp /run/secrets/github_credentials /run/bootstrap/github_credentials
chown 1000:1000 /run/bootstrap/github_credentials
chmod 600 /run/bootstrap/github_credentials
chown 1000:1000 /run/bootstrap/jenkins_password "$JENKINS_HOME/init.groovy.d" "$JENKINS_HOME/init.groovy.d/10-mainframe-env.groovy"
chmod 600 /run/bootstrap/jenkins_password
socket_gid="$(stat -c '%g' /var/run/docker.sock)"
exec setpriv --reuid=1000 --regid=1000 --groups="$socket_gid" \
  java ${JAVA_OPTS:--Xmx1024m} -Djenkins.install.runSetupWizard=false \
  -Djava.io.tmpdir=/state/tmp \
  -Dhudson.plugins.git.GitSCM.ALLOW_LOCAL_CHECKOUT=true \
  -jar "$JENKINS_HOME/controller/jenkins.war" --httpListenAddress=0.0.0.0 --httpPort=8080
