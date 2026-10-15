package com.notemaker;

import android.app.AlertDialog;
import android.graphics.*;
import android.graphics.drawable.GradientDrawable;
import android.text.Editable;
import android.text.TextWatcher;
import android.view.ViewGroup;
import android.view.View;
import android.widget.*;
import com.notemaker.core.*;
import java.util.*;
import java.util.concurrent.*;

/** Browses the selected replica; all writes use its existing document commands. */
final class DocumentBrowser {
    private final MainActivity app;
    private final AppReplica source;
    private final List<String> names = new ArrayList<>(), visible = new ArrayList<>();
    private AlertDialog dialog;
    private BaseAdapter adapter;
    private GridView files;
    private boolean gridMode;
    private volatile boolean closed;
    private volatile int generation;
    private final ExecutorService previews=Executors.newSingleThreadExecutor();
    private final android.util.LruCache<String,Bitmap> thumbnails=new android.util.LruCache<>(24);
    private final Set<String> loading=new HashSet<>(), failed=new HashSet<>();
    private TextView status;
    private EditText search;
    private boolean loaded;

    DocumentBrowser(MainActivity app){this.app=app;source=app.replica;}

    void show(){
        LinearLayout body=new LinearLayout(app);body.setOrientation(1);
        int padding=app.editor.dp(20);body.setPadding(padding,0,padding,0);
        TextView scope=new TextView(app);
        scope.setText(source.remote()?"Files on the selected editing proxy":"Files on this device · available offline");body.addView(scope);
        search=new EditText(app);search.setSingleLine();search.setHint("Search files");body.addView(search);
        gridMode=app.getSharedPreferences("settings",0).getBoolean("filesGrid",true);
        RadioGroup modes=new RadioGroup(app);modes.setOrientation(LinearLayout.HORIZONTAL);
        RadioButton grid=new RadioButton(app);grid.setId(View.generateViewId());grid.setText("Grid");modes.addView(grid);
        RadioButton list=new RadioButton(app);list.setId(View.generateViewId());list.setText("List");modes.addView(list);
        modes.check(gridMode?grid.getId():list.getId());body.addView(modes);
        status=new TextView(app);body.addView(status);
        files=new GridView(app);files.setStretchMode(GridView.STRETCH_COLUMN_WIDTH);
        files.setColumnWidth(app.editor.dp(150));files.setHorizontalSpacing(app.editor.dp(12));files.setVerticalSpacing(app.editor.dp(12));
        adapter=new BaseAdapter(){
            public int getCount(){return visible.size();}
            public Object getItem(int position){return visible.get(position);}
            public long getItemId(int position){return position;}
            public View getView(int position,View recycled,ViewGroup parent){return fileView(visible.get(position));}
        };
        files.setAdapter(adapter);displayMode();
        modes.setOnCheckedChangeListener((group,id)->{gridMode=id==grid.getId();app.getSharedPreferences("settings",0).edit().putBoolean("filesGrid",gridMode).apply();displayMode();});
        body.addView(files,new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT,app.editor.dp(360)));
        files.setOnItemClickListener((parent,view,position,id)->{
            String name=visible.get(position);dialog.dismiss();app.openDocument(name,"fixed");
        });
        search.addTextChangedListener(new TextWatcher(){
            public void beforeTextChanged(CharSequence s,int start,int count,int after){}
            public void onTextChanged(CharSequence s,int start,int before,int count){filter();}
            public void afterTextChanged(Editable s){}
        });
        dialog=new AlertDialog.Builder(app).setTitle("Files").setView(body)
            .setPositiveButton("New file",null).setNeutralButton("Refresh",null).setNegativeButton("Close",null).create();
        dialog.setOnDismissListener(d->{closed=true;previews.shutdownNow();thumbnails.evictAll();});
        dialog.setOnShowListener(d->{
            dialog.getButton(AlertDialog.BUTTON_POSITIVE).setOnClickListener(v->create());
            dialog.getButton(AlertDialog.BUTTON_NEUTRAL).setOnClickListener(v->refresh());refresh();
        });dialog.show();
    }

    private boolean current(){return dialog.isShowing()&&!app.isFinishing()&&app.replica==source;}

    private void displayMode(){
        files.setNumColumns(gridMode?GridView.AUTO_FIT:1);
        files.setContentDescription(gridMode?"Files grid":"Files list");adapter.notifyDataSetChanged();
    }

    private View fileView(String name){
        LinearLayout tile=new LinearLayout(app);tile.setOrientation(LinearLayout.VERTICAL);
        int pad=app.editor.dp(8);tile.setPadding(pad,pad,pad,pad);
        GradientDrawable border=new GradientDrawable();border.setColor(Color.WHITE);border.setStroke(app.editor.dp(1),Color.LTGRAY);tile.setBackground(border);
        if(gridMode){
            Bitmap bitmap=thumbnails.get(name);
            if(bitmap!=null){
                ImageView image=new ImageView(app);image.setImageBitmap(bitmap);image.setScaleType(ImageView.ScaleType.FIT_CENTER);
                image.setContentDescription("Preview of "+name);tile.addView(image,new LinearLayout.LayoutParams(-1,app.editor.dp(160)));
            }else{
                TextView placeholder=new TextView(app);placeholder.setGravity(android.view.Gravity.CENTER);
                placeholder.setText(failed.contains(name)?"Preview unavailable":"Loading preview…");tile.addView(placeholder,new LinearLayout.LayoutParams(-1,app.editor.dp(160)));
                if(!failed.contains(name))preview(name);
            }
        }
        TextView label=new TextView(app);label.setText(name);label.setTextColor(Color.BLACK);label.setTextSize(16);
        label.setMaxLines(2);label.setEllipsize(android.text.TextUtils.TruncateAt.END);label.setMinHeight(app.editor.dp(48));
        label.setGravity(android.view.Gravity.CENTER_VERTICAL);tile.addView(label);return tile;
    }

    private void preview(String name){
        if(closed||!loading.add(name))return;
        final int request=generation;
        previews.execute(()->{
            if(closed||request!=generation)return;
            Bitmap bitmap=null;
            try{bitmap=thumbnail(Json.map(source.command(name,Arrays.asList("snapshot"),null)));}
            catch(Exception e){android.util.Log.w("NoteMaker","File preview unavailable",e);}
            final Bitmap result=bitmap;
            app.runOnUiThread(()->{
                if(closed||request!=generation)return;
                loading.remove(name);if(result==null)failed.add(name);else thumbnails.put(name,result);
                if(current()&&gridMode)adapter.notifyDataSetChanged();
            });
        });
    }

    static Bitmap thumbnail(Map<String,Object> state){
        Model model=Store.model(state);Set<String> hidden=new HashSet<>();
        for(Object layer:Json.list(state.getOrDefault("hidden",Collections.emptyList())))hidden.add(String.valueOf(layer));
        double left=0,top=0,right=1000,bottom=1400;
        if("infinite".equals(model.type)){
            boolean found=false;
            for(String layer:model.layers.keySet()){
                if(Boolean.FALSE.equals(model.layerActive.get(layer))||(hidden.contains(layer)&&!Boolean.TRUE.equals(model.layers.get(layer).get("force"))))continue;
                for(Map<String,Object> content:model.layerItems(layer,0)){
                    DrawItem item=new DrawItem(model,content);if(!item.active())continue;
                    double[] b=item.paintBounds(com.notemaker.cli.ItemCoverage::create);
                    if(!found){left=b[0];top=b[1];right=b[2];bottom=b[3];found=true;}
                    else{left=Math.min(left,b[0]);top=Math.min(top,b[1]);right=Math.max(right,b[2]);bottom=Math.max(bottom,b[3]);}
                }
            }
            if(found){left-=30;top-=30;right+=30;bottom+=30;}
        }
        Bitmap bitmap=Bitmap.createBitmap(240,336,Bitmap.Config.ARGB_8888);Canvas canvas=new Canvas(bitmap);canvas.drawColor(Color.WHITE);
        float scale=(float)Math.min(232/Math.max(1,right-left),328/Math.max(1,bottom-top));
        canvas.translate((240-(float)(right-left)*scale)/2,(336-(float)(bottom-top)*scale)/2);
        canvas.scale(scale,scale);canvas.translate((float)-left,(float)-top);
        Render.draw(model,0,hidden,new com.notemaker.cli.Png(canvas));return bitmap;
    }

    private void filter(){
        visible.clear();String query=search.getText().toString().trim().toLowerCase(Locale.ROOT);
        for(String name:names)if(name.toLowerCase(Locale.ROOT).contains(query))visible.add(name);
        adapter.notifyDataSetChanged();
        if(loaded)status.setText(names.isEmpty()?"No files yet. Choose New file to begin.":visible.isEmpty()?"No matching files.":visible.size()+" files · tap to open");
    }

    private void refresh(){
        generation++;thumbnails.evictAll();failed.clear();loading.clear();
        loaded=false;status.setText("Loading files…");dialog.getButton(AlertDialog.BUTTON_NEUTRAL).setEnabled(false);
        ReplicaService.writes.execute(()->{
            try{
                List<String> result=new ArrayList<>();
                for(Object name:Json.list(Json.map(source.command(Arrays.asList("document","list"),null)).get("documents")))result.add(String.valueOf(name));
                result.sort(String.CASE_INSENSITIVE_ORDER.thenComparing(Comparator.naturalOrder()));
                app.runOnUiThread(()->{if(!current())return;names.clear();names.addAll(result);loaded=true;filter();dialog.getButton(AlertDialog.BUTTON_NEUTRAL).setEnabled(true);});
            }catch(Exception e){app.runOnUiThread(()->{if(!current())return;status.setText("Could not load files. Check the editing proxy and tap Refresh.");dialog.getButton(AlertDialog.BUTTON_NEUTRAL).setEnabled(true);});}
        });
    }

    private void create(){
        LinearLayout fields=new LinearLayout(app);fields.setOrientation(1);
        int padding=app.editor.dp(20);fields.setPadding(padding,0,padding,0);
        EditText name=new EditText(app);name.setSingleLine();name.setHint("File name");fields.addView(name);
        Spinner type=new Spinner(app);type.setAdapter(new ArrayAdapter<>(app,android.R.layout.simple_spinner_dropdown_item,new String[]{"Fixed page","Multiple pages","Infinite canvas"}));fields.addView(type);
        TextView error=new TextView(app);fields.addView(error);
        AlertDialog create=new AlertDialog.Builder(app).setTitle("New file").setView(fields).setPositiveButton("Create",null).setNegativeButton("Cancel",null).create();
        create.setOnShowListener(d->create.getButton(AlertDialog.BUTTON_POSITIVE).setOnClickListener(v->{
            String next=name.getText().toString().trim();
            if(next.isEmpty()||next.length()>200){error.setText("Enter a file name between 1 and 200 characters");return;}
            String kind=new String[]{"fixed","pages","infinite"}[type.getSelectedItemPosition()];
            create.getButton(AlertDialog.BUTTON_POSITIVE).setEnabled(false);
            create.getButton(AlertDialog.BUTTON_NEGATIVE).setEnabled(false);create.setCancelable(false);
            ReplicaService.writes.execute(()->{
                try{
                    AppReplica target=new AppReplica(app,next,app.proxyUrl,app.proxyToken);
                    target.command(Arrays.asList("document","create",kind),null);
                    app.runOnUiThread(()->{create.dismiss();if(!current())return;dialog.dismiss();app.openDocument(next,kind);});
                }catch(Exception e){app.runOnUiThread(()->{
                    if(!create.isShowing())return;
                    error.setText("Could not create file: "+e.getMessage());
                    create.getButton(AlertDialog.BUTTON_POSITIVE).setEnabled(true);
                    create.getButton(AlertDialog.BUTTON_NEGATIVE).setEnabled(true);create.setCancelable(true);
                });}
            });
        }));create.show();
    }
}
