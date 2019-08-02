package dev.nhh.echo.client;

import dev.nhh.echo.client.util.ConnectionList;
import javafx.application.Application;
import javafx.fxml.FXMLLoader;
import javafx.scene.Parent;
import javafx.scene.Scene;
import javafx.scene.image.Image;
import javafx.stage.Stage;

public class Client extends Application {

    @Override
    public void start(Stage stage) throws Exception {
        Parent root = FXMLLoader.load(getClass().getResource("/layout/layout.fxml"));

        Scene scene = new Scene(root);
        scene.getStylesheets().add(getClass().getResource("/theme/dark.css").toExternalForm());
        stage.getIcons().add(new Image(getClass().getResourceAsStream("/icon.png")));

        stage.setScene(scene);
        stage.show();

        stage.setOnCloseRequest(event -> ConnectionList.INSTANCE.stopAllConnections());
    }

    public static void main(String[] args) {
       launch();
    }

}
