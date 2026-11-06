-- V3: retainer lines are not revenue (BR-SALES-17): the view counted them, so a retainer deposit read as revenue. A
-- transaction whose lines are all retainer lines no longer counts in `transactions`.
DROP VIEW sales_summary_by_month;
CREATE VIEW sales_summary_by_month AS
  SELECT st.period AS period, SUM(sl.amount) AS revenue, COUNT(DISTINCT st.id) AS transactions
  FROM sales_transaction st JOIN sales_line sl ON sl.sales_transaction_id = st.id
  LEFT JOIN item it ON it.id = sl.item_id
  WHERE st.locked_ind = 1 AND COALESCE(st.deleted, 0) = 0 AND COALESCE(it.name, '') <> 'Retainer'
  GROUP BY st.period;
