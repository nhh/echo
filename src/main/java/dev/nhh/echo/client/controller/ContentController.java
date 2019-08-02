package dev.nhh.echo.client.controller;

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
    private Button button;

    @FXML
    void click() {
        button.setText("Hallo Welt" + new Date().toString());
    }

    @FXML
    protected void stop(MouseEvent event) {
        ConnectionList.INSTANCE.stopAllConnections();
    }

    @FXML
    protected void start(MouseEvent event) {
        var connection = new Connection(textField.getText());
        connection.start();
        ConnectionList.INSTANCE.addConnection(connection);
    }
}
