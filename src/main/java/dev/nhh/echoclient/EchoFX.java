package dev.nhh.echoclient;

import dev.nhh.echoclient.network.Connection;
import dev.nhh.echoclient.util.ThreadScheduler;
import javafx.application.Application;
import javafx.fxml.FXMLLoader;
import javafx.scene.Parent;
import javafx.scene.Scene;
import javafx.scene.image.Image;
import javafx.stage.Stage;

public class EchoFX extends Application {

    @Override
    public void start(Stage stage) throws Exception {
        Parent root = FXMLLoader.load(getClass().getResource("/layout/layout.fxml"));

        Scene scene = new Scene(root);
        scene.getStylesheets().add(getClass().getResource("/theme/dark.css").toExternalForm());
        stage.getIcons().add(new Image(getClass().getResourceAsStream("/icon.png")));

        stage.setScene(scene);
        stage.show();

        var connectionThread = new Thread(new Connection());
        connectionThread.start();

        stage.setOnCloseRequest(event -> ThreadScheduler.INSTANCE.stopAll());

    }

    public static void main(String[] args) {
       launch();
    }

}
