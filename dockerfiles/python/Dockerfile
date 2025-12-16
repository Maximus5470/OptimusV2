# Python Execution Environment
FROM python:3.11-slim

WORKDIR /code

# Install common packages
RUN pip install --no-cache-dir \
    pytest \
    numpy \
    requests

# Default command (will be overridden)
CMD ["python", "--version"]
