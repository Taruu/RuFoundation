#!/bin/sh

echo Starting migrations...
python manage.py migrate

echo Starting server...
gunicorn scpdev.wsgi -w 4 -t 300 -b 0.0.0.0:7777 --preload