import com.android.build.api.dsl.ApplicationExtension;
import com.android.build.api.dsl.ProductFlavor;
import java.util.Arrays;
import java.util.Collection;
import java.util.List;
import org.gradle.api.DefaultTask;
import org.gradle.api.Plugin;
import org.gradle.api.Project;
import org.gradle.api.Task;

/** Adds a product flavor per Android ABI and the tasks that build Rust for each. */
public class RustPlugin implements Plugin<Project> {
    private static final String TASK_GROUP = "rust";
    private static final List<String> DEFAULT_ABIS = List.of("arm64-v8a", "armeabi-v7a", "x86", "x86_64");
    private static final List<String> DEFAULT_ARCHES = List.of("arm64", "arm", "x86", "x86_64");
    private static final List<String> DEFAULT_TARGETS = List.of("aarch64", "armv7", "i686", "x86_64");

    @Override
    public void apply(Project project) {
        RustConfig config = project.getExtensions().create("rust", RustConfig.class);
        List<String> abis = listProperty(project, "abiList", DEFAULT_ABIS);
        List<String> arches = listProperty(project, "archList", DEFAULT_ARCHES);
        List<String> targets = listProperty(project, "targetList", DEFAULT_TARGETS);

        ApplicationExtension android = project.getExtensions().getByType(ApplicationExtension.class);
        android.getFlavorDimensions().add("abi");
        android.getProductFlavors().create("universal", flavor -> configureFlavor(flavor, abis));
        for (int index = 0; index < DEFAULT_ARCHES.size(); index++) {
            List<String> abi = List.of(DEFAULT_ABIS.get(index));
            android.getProductFlavors().create(DEFAULT_ARCHES.get(index), flavor -> configureFlavor(flavor, abi));
        }

        project.afterEvaluate(evaluated -> {
            for (String profile : List.of("debug", "release")) {
                addTasks(evaluated, config, profile, arches, targets);
            }
        });
    }

    // With one flavor dimension, Android assigns it to every flavor. Setting it here is not
    // possible from Java anyway: the plugin declares setDimension twice, so the call is ambiguous.
    private static void configureFlavor(ProductFlavor flavor, Collection<String> abis) {
        flavor.getNdk().getAbiFilters().addAll(abis);
    }

    private static void addTasks(Project project, RustConfig config, String profile, List<String> arches, List<String> targets) {
        String profileName = capitalize(profile);
        Task universal = project.getTasks().maybeCreate("rustBuildUniversal" + profileName, DefaultTask.class);
        universal.setGroup(TASK_GROUP);
        universal.setDescription("Build dynamic library in " + profile + " mode for all targets");
        project.getTasks().getByName("mergeUniversal" + profileName + "JniLibFolders").dependsOn(universal);

        for (int index = 0; index < targets.size(); index++) {
            String arch = arches.get(index);
            String archName = capitalize(arch);
            BuildTask build = project.getTasks().maybeCreate("rustBuild" + archName + profileName, BuildTask.class);
            build.setGroup(TASK_GROUP);
            build.setDescription("Build dynamic library in " + profile + " mode for " + arch);
            build.setRootDirRel(config.getRootDirRel());
            build.setTarget(targets.get(index));
            build.setRelease(profile.equals("release"));
            universal.dependsOn(build);
            project.getTasks().getByName("merge" + archName + profileName + "JniLibFolders").dependsOn(build);
        }
    }

    private static List<String> listProperty(Project project, String name, List<String> fallback) {
        Object value = project.findProperty(name);
        return value instanceof String text ? Arrays.asList(text.split(",")) : fallback;
    }

    private static String capitalize(String text) {
        return text.substring(0, 1).toUpperCase() + text.substring(1);
    }
}
