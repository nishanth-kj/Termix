import java.io.File;
import java.util.ArrayList;
import java.util.List;
import org.apache.tools.ant.taskdefs.condition.Os;
import org.gradle.api.DefaultTask;
import org.gradle.api.GradleException;
import org.gradle.api.logging.LogLevel;
import org.gradle.api.tasks.Input;
import org.gradle.api.tasks.TaskAction;

/** Builds the Rust library for one target through the Tauri CLI. */
public class BuildTask extends DefaultTask {
    private String rootDirRel;
    private String target;
    private boolean release;

    @Input
    public String getRootDirRel() {
        return rootDirRel;
    }

    public void setRootDirRel(String rootDirRel) {
        this.rootDirRel = rootDirRel;
    }

    @Input
    public String getTarget() {
        return target;
    }

    public void setTarget(String target) {
        this.target = target;
    }

    @Input
    public boolean isRelease() {
        return release;
    }

    public void setRelease(boolean release) {
        this.release = release;
    }

    @TaskAction
    public void assemble() {
        String executable = "cargo";
        try {
            runTauriCli(executable);
        } catch (RuntimeException failure) {
            if (!Os.isFamily(Os.FAMILY_WINDOWS)) {
                throw failure;
            }
            RuntimeException last = failure;
            for (String extension : List.of(".exe", ".cmd", ".bat")) {
                try {
                    runTauriCli(executable + extension);
                    return;
                } catch (RuntimeException next) {
                    last = next;
                }
            }
            throw last;
        }
    }

    private void runTauriCli(String executable) {
        if (rootDirRel == null || target == null) {
            throw new GradleException("rootDirRel and target are required");
        }
        List<String> arguments = new ArrayList<>(List.of("tauri", "android", "android-studio-script"));
        if (getProject().getLogger().isEnabled(LogLevel.DEBUG)) {
            arguments.add("-vv");
        } else if (getProject().getLogger().isEnabled(LogLevel.INFO)) {
            arguments.add("-v");
        }
        if (release) {
            arguments.add("--release");
        }
        arguments.add("--target");
        arguments.add(target);
        getProject()
                .exec(spec -> {
                    spec.workingDir(new File(getProject().getProjectDir(), rootDirRel));
                    spec.executable(executable);
                    spec.args(arguments);
                })
                .assertNormalExitValue();
    }
}
