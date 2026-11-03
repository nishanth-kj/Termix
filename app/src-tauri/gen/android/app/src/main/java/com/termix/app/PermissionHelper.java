package com.termix.app;

import android.content.Context;
import android.content.pm.PackageInfo;
import android.content.pm.PackageManager;
import android.os.Build;
import androidx.core.app.ActivityCompat;
import java.util.ArrayList;
import java.util.Arrays;

public final class PermissionHelper {
    private PermissionHelper() {}

    public static boolean hasPermissions(Context context, String[] permissions) {
        for (String permission : permissions) {
            if (ActivityCompat.checkSelfPermission(context, permission) != PackageManager.PERMISSION_GRANTED) {
                return false;
            }
        }
        return true;
    }

    public static boolean hasDefinedPermission(Context context, String permission) {
        String[] requestedPermissions = getManifestPermissions(context);
        if (requestedPermissions == null || requestedPermissions.length == 0) {
            return false;
        }
        return new ArrayList<>(Arrays.asList(requestedPermissions)).contains(permission);
    }

    public static boolean hasDefinedPermissions(Context context, String[] permissions) {
        for (String permission : permissions) {
            if (!hasDefinedPermission(context, permission)) {
                return false;
            }
        }
        return true;
    }

    @SuppressWarnings("deprecation")
    private static String[] getManifestPermissions(Context context) {
        try {
            PackageManager manager = context.getPackageManager();
            PackageInfo packageInfo;
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                packageInfo = manager.getPackageInfo(
                        context.getPackageName(),
                        PackageManager.PackageInfoFlags.of(PackageManager.GET_PERMISSIONS));
            } else {
                packageInfo = manager.getPackageInfo(context.getPackageName(), PackageManager.GET_PERMISSIONS);
            }
            if (packageInfo != null) {
                return packageInfo.requestedPermissions;
            }
        } catch (Exception ignored) {
        }
        return null;
    }

    public static String[] getUndefinedPermissions(Context context, String[] neededPermissions) {
        String[] requestedPermissions = getManifestPermissions(context);
        if (requestedPermissions == null || requestedPermissions.length == 0) {
            return neededPermissions;
        }
        ArrayList<String> requested = new ArrayList<>(Arrays.asList(requestedPermissions));
        ArrayList<String> undefined = new ArrayList<>();
        for (String permission : neededPermissions) {
            if (!requested.contains(permission)) {
                undefined.add(permission);
            }
        }
        return undefined.toArray(new String[0]);
    }
}
