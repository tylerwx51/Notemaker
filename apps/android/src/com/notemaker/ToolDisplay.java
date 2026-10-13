package com.notemaker;

import org.json.*;
import java.util.*;

/** Human-readable tool inputs; output remains verbatim in the expandable row. */
final class ToolDisplay {
    final String summary, details;

    ToolDisplay(String name, String raw) {
        JSONObject args;
        try { args = new JSONObject(raw); }
        catch (JSONException error) {
            summary = name;
            details = raw.isEmpty() ? "No arguments" : raw;
            return;
        }
        String path = args.optString("path", args.optString("file_path", args.optString("file", "")));
        String primary = "";
        StringBuilder description = new StringBuilder();
        Set<String> used = new HashSet<>();
        switch (name) {
            case "read": case "write": case "edit": case "ls":
                primary = path;
                description.append(path.isEmpty() ? "." : path);
                used.addAll(Arrays.asList("path", "file_path", "file"));
                break;
            case "bash": case "powershell":
                primary = args.optString("command");
                description.append(primary);
                used.add("command");
                break;
            case "grep": case "find":
                primary = args.optString("pattern") + (path.isEmpty() ? "" : " in " + path);
                description.append("Pattern: ").append(args.optString("pattern"));
                if (!path.isEmpty()) description.append("\nIn: ").append(path);
                used.addAll(Arrays.asList("pattern", "path", "file_path", "file"));
                break;
            case "mock.inspect":
                primary = args.optString("document");
                break;
            default:
                primary = path;
                break;
        }
        Iterator<String> keys = args.keys();
        while (keys.hasNext()) {
            String key = keys.next();
            if (used.contains(key)) continue;
            Object value = args.opt(key);
            if (description.length() > 0) description.append("\n\n");
            String label;
            switch (key) {
                case "oldText": label = "Replace"; break;
                case "newText": label = "With"; break;
                case "content": label = "Content"; break;
                case "offset": label = "Starting line"; break;
                case "limit": label = "Limit"; break;
                case "timeout": label = "Timeout (seconds)"; break;
                default:
                    label = key.replace('_', ' ').replaceAll("([a-z])([A-Z])", "$1 $2");
                    if (!label.isEmpty()) label = Character.toUpperCase(label.charAt(0)) + label.substring(1);
            }
            String formatted = String.valueOf(value);
            try {
                if (value instanceof JSONObject) formatted = ((JSONObject)value).toString(2);
                if (value instanceof JSONArray) formatted = ((JSONArray)value).toString(2);
            } catch (JSONException ignored) {}
            description.append(label).append(formatted.contains("\n") || key.equals("content") || key.equals("oldText") || key.equals("newText") ? ":\n" : ": ").append(formatted);
        }
        primary = primary.replaceAll("\\s+", " ").trim();
        if (primary.length() > 160) primary = primary.substring(0, 157) + "…";
        summary = name + (primary.isEmpty() ? "" : " " + primary);
        details = description.length() == 0 ? "No arguments" : description.toString();
    }
}
