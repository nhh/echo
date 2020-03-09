package dev.nhh.echo.client.controller;

import com.sun.javafx.scene.control.IntegerField;
import dev.nhh.echo.client.audio.speaker.GainControl;
import dev.nhh.echo.client.util.Connection;
import dev.nhh.echo.client.util.ConnectionList;
import javafx.fxml.FXML;
import javafx.scene.control.Button;
import javafx.scene.control.TextField;
import javafx.scene.input.MouseEvent;

import java.util.Date;

public class ContentController {

    @FXML
    private TextField textField;

    @FXML
    private TextField volume;

    @FXML
    private Button button;

    @FXML
    private Button applyVolume;

    @FXML
    protected void applyVolume(MouseEvent event) {
        GainControl.INSTANCE.setVolume(Float.parseFloat(volume.getText()));
    }

    @FXML
    protected void stop(MouseEvent event) {
        ConnectionList.INSTANCE.stopAllConnections();
    }

    @FXML
    protected void start(MouseEvent event) {
        Connection connection = new Connection(textField.getText());
        connection.start();
        ConnectionList.INSTANCE.addConnection(connection);
    }
}
