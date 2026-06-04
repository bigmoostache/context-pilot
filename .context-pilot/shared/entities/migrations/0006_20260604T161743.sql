CREATE TRIGGER log_company_insert AFTER INSERT ON companies
BEGIN
    INSERT INTO audit_log (action, table_name, row_id) VALUES ('INSERT', 'companies', NEW.id);
END