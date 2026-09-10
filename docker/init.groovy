import hudson.security.FullControlOnceLoggedInAuthorizationStrategy
import hudson.security.HudsonPrivateSecurityRealm
import hudson.security.csrf.DefaultCrumbIssuer
import hudson.triggers.SCMTrigger
import jenkins.model.Jenkins
import jenkins.model.JenkinsLocationConfiguration
import org.jenkinsci.plugins.workflow.cps.CpsFlowDefinition
import org.jenkinsci.plugins.workflow.job.WorkflowJob
import org.jenkinsci.plugins.workflow.job.properties.PipelineTriggersJobProperty
import com.cloudbees.plugins.credentials.CredentialsScope
import com.cloudbees.plugins.credentials.SystemCredentialsProvider
import com.cloudbees.plugins.credentials.domains.Domain
import com.cloudbees.plugins.credentials.impl.UsernamePasswordCredentialsImpl
import groovy.json.JsonSlurper

def controller = Jenkins.get()
controller.setNumExecutors(1)
controller.setSlaveAgentPort(-1)
controller.setCrumbIssuer(new DefaultCrumbIssuer(true))
if (!(controller.getSecurityRealm() instanceof HudsonPrivateSecurityRealm)) {
    def realm = new HudsonPrivateSecurityRealm(false)
    realm.createAccount('admin', new File('/run/bootstrap/jenkins_password').text.trim())
    controller.setSecurityRealm(realm)
    def authorization = new FullControlOnceLoggedInAuthorizationStrategy()
    authorization.setAllowAnonymousRead(false)
    controller.setAuthorizationStrategy(authorization)
}
JenkinsLocationConfiguration.get().setUrl('http://127.0.0.1:18080/')
JenkinsLocationConfiguration.get().save()
def credential = new JsonSlurper().parse(new File('/run/bootstrap/github_credentials'))
def provider = SystemCredentialsProvider.getInstance()
def credentialId = 'mainframe-env-github'
if (credential.password && !provider.getCredentials().any { it.id == credentialId }) {
    provider.getStore().addCredentials(Domain.global(), new UsernamePasswordCredentialsImpl(
        CredentialsScope.GLOBAL, credentialId, 'GitHub repository checkout',
        credential.username as String, credential.password as String))
}
def job = controller.getItem('mainframe-env-local')
def firstStart = job == null
if (firstStart) {
    job = controller.createProject(WorkflowJob, 'mainframe-env-local')
}
job.setDefinition(new CpsFlowDefinition(new File('/opt/mainframe-env/docker/Jenkinsfile').text, true))
job.setDescription('Main-branch CI and health-checked local Docker deployment. Full assurance remains in the repository Jenkinsfile.')
job.addProperty(new PipelineTriggersJobProperty([new SCMTrigger('H/5 * * * *')]))
job.save()
controller.save()
if (firstStart) {
    job.scheduleBuild2(0)
}
