import jenkins.model.Jenkins

def controller = Jenkins.get()
if (controller.getNumExecutors() != 1) {
    controller.setNumExecutors(1)
    controller.save()
}
