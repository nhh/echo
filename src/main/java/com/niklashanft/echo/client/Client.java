package com.niklashanft.echo.client;

import com.niklashanft.echo.client.util.Connection;
import javafx.application.Application;
import javafx.application.Platform;
import javafx.beans.value.ChangeListener;
import javafx.concurrent.Worker;
import javafx.scene.Scene;
import javafx.scene.image.Image;
import javafx.scene.layout.VBox;
import javafx.scene.web.WebView;
import javafx.stage.Stage;
import netscape.javascript.JSObject;

public class Client extends Application {

    // Example class callable from javascript
    public static class MyObject {
        public void doIt() {
            System.out.println("doIt() called");
        }
    }

    @Override
    public void start(Stage stage) {
        stage.setTitle("JavaFX WebView Example");

        WebView webView = new WebView();

        // When developing, use live server. When production use bundled file url
        //URL url = getClass().getResource("/index.html");
        //webView.getEngine().load(url.toString());
        webView.getEngine().load("http://localhost:3000");

        // PoC for providing Java functions as Javascript objects.
        webView.getEngine().getLoadWorker().stateProperty().addListener(
                (ChangeListener) (observable, oldValue, newValue) -> {
                    if (newValue != Worker.State.SUCCEEDED) {
                        return;
                    }

                    JSObject window = (JSObject) webView.getEngine().executeScript("window");
                    window.setMember("myObject", new MyObject());
                }
        );

        // PoC for dispatching a event from java to js.
        var t = new Thread(() -> {
            try {
                while (true) {
                    Thread.sleep(500);
                    Platform.runLater(() -> webView.getEngine().executeScript("window.dispatchEvent(new Event('echo'));"));
                }
            } catch (InterruptedException e) {
                e.printStackTrace();
            }
        });

        t.start();

        // Starting the actual "connection". Atm its a stupid name. But we have to put it somewhere.
        var t2 = new Thread(() -> {
            var conn = new Connection("localhost");
            while(true) {
                try {
                    Thread.sleep(16); // Todo Check in what speed we actually want to send voice data
                    conn.send();
                } catch (InterruptedException e) {
                    e.printStackTrace();
                }
            }
        });

        t2.start();

        // JavaFX Stuff.
        VBox vBox = new VBox(webView);
        Scene scene = new Scene(vBox, 960, 600);

        stage.getIcons().add(new Image(getClass().getResourceAsStream("/icon.png")));

        stage.setScene(scene);
        stage.show();
    }

    public static void main(String[] args) {
        launch();
    }

}
