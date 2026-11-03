package com.termix.app;

import android.Manifest;
import android.app.Activity;
import android.app.AlertDialog;
import android.content.ActivityNotFoundException;
import android.content.Intent;
import android.graphics.Bitmap;
import android.net.Uri;
import android.os.Build;
import android.os.Environment;
import android.provider.MediaStore;
import android.view.View;
import android.webkit.GeolocationPermissions;
import android.webkit.JsPromptResult;
import android.webkit.JsResult;
import android.webkit.MimeTypeMap;
import android.webkit.PermissionRequest;
import android.webkit.ValueCallback;
import android.webkit.WebChromeClient;
import android.webkit.WebView;
import android.widget.EditText;
import androidx.core.content.FileProvider;
import java.io.File;
import java.io.IOException;
import java.text.SimpleDateFormat;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Date;
import java.util.List;
import java.util.Locale;

public class RustWebChromeClient extends WebChromeClient {
    private static final int PERMISSION_REQUEST_DEFAULT = 0;
    private static final int PERMISSION_REQUEST_ALLOW = 1;
    private static final int PERMISSION_REQUEST_DENY = 2;

    private final WryActivity activity;
    private final String webViewId;

    public RustWebChromeClient(WryActivity activity, String webViewId) {
        this.activity = activity;
        this.webViewId = webViewId;
    }

    @Override
    public void onShowCustomView(View view, CustomViewCallback callback) {
        callback.onCustomViewHidden();
        super.onShowCustomView(view, callback);
    }

    @Override
    public void onHideCustomView() {
        super.onHideCustomView();
    }

    @Override
    public void onPermissionRequest(PermissionRequest request) {
        String[] requestedResources = request.getResources();
        if (requestedResources.length == 0) {
            request.deny();
            return;
        }

        ArrayList<String> allowedResources = new ArrayList<>();
        ArrayList<String> defaultResources = new ArrayList<>();
        for (String resource : requestedResources) {
            int decision = onPermissionRequestNative(webViewId, resource);
            if (decision == PERMISSION_REQUEST_ALLOW) {
                allowedResources.add(resource);
            } else if (decision == PERMISSION_REQUEST_DEFAULT) {
                defaultResources.add(resource);
            }
        }

        ArrayList<String> resources = new ArrayList<>(allowedResources);
        resources.addAll(Arrays.asList(filterKnownPermissions(defaultResources)));
        grantPermissionRequest(request, resources.toArray(new String[0]));
    }

    private void grantPermissionRequest(PermissionRequest request, String[] resources) {
        if (resources.length == 0) {
            request.deny();
            return;
        }
        boolean requestPermissionRequired = Build.VERSION.SDK_INT >= Build.VERSION_CODES.M;
        List<String> permissionList = androidPermissionsForResources(resources);
        if (!permissionList.isEmpty() && requestPermissionRequired) {
            activity.requestPermissions(permissionList.toArray(new String[0]), granted -> {
                if (Boolean.TRUE.equals(granted)) {
                    request.grant(resources);
                } else {
                    request.deny();
                }
            });
        } else {
            request.grant(resources);
        }
    }

    private static List<String> androidPermissionsForResources(String[] resources) {
        List<String> permissions = new ArrayList<>();
        List<String> requested = Arrays.asList(resources);
        if (requested.contains(PermissionRequest.RESOURCE_VIDEO_CAPTURE)) {
            permissions.add(Manifest.permission.CAMERA);
        }
        if (requested.contains(PermissionRequest.RESOURCE_AUDIO_CAPTURE)) {
            permissions.add(Manifest.permission.MODIFY_AUDIO_SETTINGS);
            permissions.add(Manifest.permission.RECORD_AUDIO);
        }
        return permissions;
    }

    private native int onPermissionRequestNative(String webviewId, String resource);

    @Override
    public Bitmap getDefaultVideoPoster() {
        return Bitmap.createBitmap(50, 50, Bitmap.Config.ARGB_8888);
    }

    private native boolean onGeolocationPermissionRequestNative(String webviewId, String origin);

    @Override
    public boolean onJsAlert(WebView view, String url, String message, JsResult result) {
        if (activity.isFinishing()) {
            return true;
        }
        new AlertDialog.Builder(view.getContext())
                .setMessage(message)
                .setPositiveButton("OK", (dialog, which) -> {
                    dialog.dismiss();
                    result.confirm();
                })
                .setOnCancelListener(dialog -> {
                    dialog.dismiss();
                    result.cancel();
                })
                .create()
                .show();
        return true;
    }

    @Override
    public boolean onJsConfirm(WebView view, String url, String message, JsResult result) {
        if (activity.isFinishing()) {
            return true;
        }
        new AlertDialog.Builder(view.getContext())
                .setMessage(message)
                .setPositiveButton("OK", (dialog, which) -> {
                    dialog.dismiss();
                    result.confirm();
                })
                .setNegativeButton("Cancel", (dialog, which) -> {
                    dialog.dismiss();
                    result.cancel();
                })
                .setOnCancelListener(dialog -> {
                    dialog.dismiss();
                    result.cancel();
                })
                .create()
                .show();
        return true;
    }

    @Override
    public boolean onJsPrompt(WebView view, String url, String message, String defaultValue, JsPromptResult result) {
        if (activity.isFinishing()) {
            return true;
        }
        EditText input = new EditText(view.getContext());
        new AlertDialog.Builder(view.getContext())
                .setMessage(message)
                .setView(input)
                .setPositiveButton("OK", (dialog, which) -> {
                    dialog.dismiss();
                    result.confirm(input.getText().toString().trim());
                })
                .setNegativeButton("Cancel", (dialog, which) -> {
                    dialog.dismiss();
                    result.cancel();
                })
                .setOnCancelListener(dialog -> {
                    dialog.dismiss();
                    result.cancel();
                })
                .create()
                .show();
        return true;
    }

    @Override
    public void onGeolocationPermissionsShowPrompt(String origin, GeolocationPermissions.Callback callback) {
        super.onGeolocationPermissionsShowPrompt(origin, callback);
        if (onGeolocationPermissionRequestNative(webViewId, origin)) {
            callback.invoke(origin, false, false);
            return;
        }

        Logger.debug("onGeolocationPermissionsShowPrompt: DOING IT HERE FOR ORIGIN: " + origin);
        String[] geoPermissions = definedGeolocationPermissions();
        if (geoPermissions.length == 0) {
            callback.invoke(origin, false, false);
            return;
        }
        if (!PermissionHelper.hasPermissions(activity, geoPermissions)) {
            activity.requestPermissions(geoPermissions, granted -> {
                if (Boolean.TRUE.equals(granted)) {
                    callback.invoke(origin, true, false);
                    return;
                }
                String[] coarsePermission = new String[] {Manifest.permission.ACCESS_COARSE_LOCATION};
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S
                        && PermissionHelper.hasPermissions(activity, coarsePermission)) {
                    callback.invoke(origin, true, false);
                } else {
                    callback.invoke(origin, false, false);
                }
            });
        } else {
            callback.invoke(origin, true, false);
            Logger.debug("onGeolocationPermissionsShowPrompt: has required permission");
        }
    }

    private static String[] filterKnownPermissions(List<String> resources) {
        ArrayList<String> known = new ArrayList<>();
        for (String resource : resources) {
            if (PermissionRequest.RESOURCE_AUDIO_CAPTURE.equals(resource)
                    || PermissionRequest.RESOURCE_VIDEO_CAPTURE.equals(resource)
                    || PermissionRequest.RESOURCE_PROTECTED_MEDIA_ID.equals(resource)
                    || PermissionRequest.RESOURCE_MIDI_SYSEX.equals(resource)) {
                known.add(resource);
            }
        }
        return known.toArray(new String[0]);
    }

    private String[] definedGeolocationPermissions() {
        ArrayList<String> permissions = new ArrayList<>();
        if (PermissionHelper.hasDefinedPermission(activity, Manifest.permission.ACCESS_COARSE_LOCATION)) {
            permissions.add(Manifest.permission.ACCESS_COARSE_LOCATION);
        }
        if (PermissionHelper.hasDefinedPermission(activity, Manifest.permission.ACCESS_FINE_LOCATION)) {
            permissions.add(Manifest.permission.ACCESS_FINE_LOCATION);
        }
        return permissions.toArray(new String[0]);
    }

    @Override
    public boolean onShowFileChooser(
            WebView webView, ValueCallback<Uri[]> filePathCallback, FileChooserParams fileChooserParams) {
        List<String> acceptTypes = Arrays.asList(fileChooserParams.getAcceptTypes());
        boolean captureEnabled = fileChooserParams.isCaptureEnabled();
        boolean capturePhoto = captureEnabled && acceptTypes.contains("image/*");
        boolean captureVideo = captureEnabled && acceptTypes.contains("video/*");
        if (capturePhoto || captureVideo) {
            if (isMediaCaptureSupported()) {
                showMediaCaptureOrFilePicker(filePathCallback, fileChooserParams, captureVideo);
            } else {
                activity.requestPermissions(new String[] {Manifest.permission.CAMERA}, granted -> {
                    if (Boolean.TRUE.equals(granted)) {
                        showMediaCaptureOrFilePicker(filePathCallback, fileChooserParams, captureVideo);
                    } else {
                        Logger.warn(Logger.tags("FileChooser"), "Camera permission not granted");
                        filePathCallback.onReceiveValue(null);
                    }
                });
            }
        } else {
            showFilePicker(filePathCallback, fileChooserParams);
        }
        return true;
    }

    private boolean isMediaCaptureSupported() {
        String[] permissions = new String[] {Manifest.permission.CAMERA};
        return PermissionHelper.hasPermissions(activity, permissions)
                || !PermissionHelper.hasDefinedPermission(activity, Manifest.permission.CAMERA);
    }

    private void showMediaCaptureOrFilePicker(
            ValueCallback<Uri[]> filePathCallback, FileChooserParams fileChooserParams, boolean isVideo) {
        boolean shown = isVideo
                ? showVideoCapturePicker(filePathCallback)
                : showImageCapturePicker(filePathCallback);
        if (!shown) {
            Logger.warn(
                    Logger.tags("FileChooser"),
                    "Media capture intent could not be launched. Falling back to default file picker.");
            showFilePicker(filePathCallback, fileChooserParams);
        }
    }

    private boolean showImageCapturePicker(ValueCallback<Uri[]> filePathCallback) {
        Intent takePictureIntent = new Intent(MediaStore.ACTION_IMAGE_CAPTURE);
        if (takePictureIntent.resolveActivity(activity.getPackageManager()) == null) {
            return false;
        }
        Uri imageFileUri;
        try {
            imageFileUri = createImageFileUri();
        } catch (Exception ex) {
            Logger.error("Unable to create temporary media capture file: " + ex.getMessage());
            return false;
        }
        takePictureIntent.putExtra(MediaStore.EXTRA_OUTPUT, imageFileUri);
        activity.launchActivityForResult(takePictureIntent, result -> {
            Uri[] picked = null;
            if (result != null && result.getResultCode() == Activity.RESULT_OK) {
                picked = new Uri[] {imageFileUri};
            }
            filePathCallback.onReceiveValue(picked);
        });
        return true;
    }

    private boolean showVideoCapturePicker(ValueCallback<Uri[]> filePathCallback) {
        Intent takeVideoIntent = new Intent(MediaStore.ACTION_VIDEO_CAPTURE);
        if (takeVideoIntent.resolveActivity(activity.getPackageManager()) == null) {
            return false;
        }
        activity.launchActivityForResult(takeVideoIntent, result -> {
            Uri[] picked = null;
            if (result != null && result.getResultCode() == Activity.RESULT_OK && result.getData() != null) {
                picked = new Uri[] {result.getData().getData()};
            }
            filePathCallback.onReceiveValue(picked);
        });
        return true;
    }

    private void showFilePicker(ValueCallback<Uri[]> filePathCallback, FileChooserParams fileChooserParams) {
        Intent intent = fileChooserParams.createIntent();
        if (fileChooserParams.getMode() == FileChooserParams.MODE_OPEN_MULTIPLE) {
            intent.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true);
        }
        String intentType = intent.getType();
        if (fileChooserParams.getAcceptTypes().length > 1 || (intentType != null && intentType.startsWith("."))) {
            String[] validTypes = getValidTypes(fileChooserParams.getAcceptTypes());
            intent.putExtra(Intent.EXTRA_MIME_TYPES, validTypes);
            if (intentType != null && intentType.startsWith(".") && validTypes.length > 0) {
                intent.setType(validTypes[0]);
            }
        }
        try {
            activity.launchActivityForResult(intent, result -> {
                Uri[] picked;
                Intent resultIntent = result == null ? null : result.getData();
                int resultCode = result == null ? 0 : result.getResultCode();
                if (resultCode == Activity.RESULT_OK
                        && resultIntent != null
                        && resultIntent.getClipData() != null) {
                    int numFiles = resultIntent.getClipData().getItemCount();
                    picked = new Uri[numFiles];
                    for (int i = 0; i < numFiles; i++) {
                        picked[i] = resultIntent.getClipData().getItemAt(i).getUri();
                    }
                } else {
                    picked = FileChooserParams.parseResult(resultCode, resultIntent);
                }
                filePathCallback.onReceiveValue(picked);
            });
        } catch (ActivityNotFoundException ex) {
            filePathCallback.onReceiveValue(null);
        }
    }

    private static String[] getValidTypes(String[] currentTypes) {
        ArrayList<String> validTypes = new ArrayList<>();
        MimeTypeMap mimeTypes = MimeTypeMap.getSingleton();
        for (String mime : currentTypes) {
            if (mime.startsWith(".")) {
                String extensionMime = mimeTypes.getMimeTypeFromExtension(mime.substring(1));
                if (extensionMime != null && !validTypes.contains(extensionMime)) {
                    validTypes.add(extensionMime);
                }
            } else if (!validTypes.contains(mime)) {
                validTypes.add(mime);
            }
        }
        return validTypes.toArray(new String[0]);
    }

    @Override
    public boolean onConsoleMessage(android.webkit.ConsoleMessage consoleMessage) {
        String tag = Logger.tags("Console");
        String message = consoleMessage.message();
        if (message != null && isValidMsg(message)) {
            String formatted = String.format(
                    Locale.US,
                    "File: %s - Line %d - Msg: %s",
                    consoleMessage.sourceId(),
                    consoleMessage.lineNumber(),
                    message);
            String level = consoleMessage.messageLevel().name();
            if ("ERROR".equalsIgnoreCase(level)) {
                Logger.error(tag, formatted, null);
            } else if ("WARNING".equalsIgnoreCase(level)) {
                Logger.warn(tag, formatted);
            } else if ("TIP".equalsIgnoreCase(level)) {
                Logger.debug(tag, formatted);
            } else {
                Logger.info(tag, formatted);
            }
        }
        return true;
    }

    private static boolean isValidMsg(String message) {
        return !(message.contains("%cresult %c")
                || message.contains("%cnative %c")
                || message.equalsIgnoreCase("[object Object]")
                || message.equalsIgnoreCase("console.groupEnd"));
    }

    private Uri createImageFileUri() throws IOException {
        File photoFile = createImageFile(activity);
        return FileProvider.getUriForFile(activity, activity.getPackageName() + ".fileprovider", photoFile);
    }

    private static File createImageFile(Activity activity) throws IOException {
        String timeStamp = new SimpleDateFormat("yyyyMMdd_HHmmss", Locale.US).format(new Date());
        File storageDir = activity.getExternalFilesDir(Environment.DIRECTORY_PICTURES);
        return File.createTempFile("JPEG_" + timeStamp + "_", ".jpg", storageDir);
    }

    @Override
    public void onReceivedTitle(WebView view, String title) {
        Rust.handleReceivedTitle(((RustWebView) view).id, title);
    }
}
