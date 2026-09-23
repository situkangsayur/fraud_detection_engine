import streamlit as st
import requests
import json
import time
import pandas as pd
import altair as alt

st.set_page_config(page_title="Fraud Detection Engine Dashboard", layout="wide")

# Base URL FastAPI
BASE_URL = "http://fraud_engine:8000/api/v1"

st.title("🚀 Fraud Detection Engine Dashboard")

# Tabs utama
(
    tab_health,
    tab_user,
    tab_transaction,
    tab_policy,
    tab_rule,
    tab_process,
    tab_stats,
    tab_monitor,
) = st.tabs(
    [
        "🩺 Healthcheck",
        "👤 User Management",
        "💳 Transaction Management",
        "📜 Policy Management",
        "📏 Rule Management",
        "⚙️ Process Transaction",
        "📈 Statistics",
        "🔄 Monitor Transactions",
    ]
)


# Helper function
def safe_get_dataframe_from_api(url):
    try:
        resp = requests.get(url)
        resp.raise_for_status()
        result = resp.json()
        if result.get("status") == "success" and isinstance(result.get("data"), list):
            df = pd.DataFrame(result["data"])
            return df
        else:
            st.error("Invalid or empty data format.")
            return pd.DataFrame()
    except Exception as e:
        st.error(f"Error fetching data: {e}")
        return pd.DataFrame()


# Healthcheck Tab
with tab_health:
    st.header("🩺 API Healthcheck")
    if st.button("Check API Health"):
        try:
            response = requests.get(f"{BASE_URL}/health")
            st.success("Server is Live!")
            st.json(response.json())
        except Exception as e:
            st.error(f"Server error: {e}")

# User Management Tab
with tab_user:
    st.header("👤 User Management")
    create_user_expander = st.expander("➕ Create User")
    with create_user_expander:
        with st.form("create_user_form"):
            id_user = st.text_input("User ID")
            nama_lengkap = st.text_input("Full Name")
            email = st.text_input("Email")
            domain_email = st.text_input("Domain Email")
            address = st.text_input("Address")
            address_zip = st.text_input("Zip Code")
            address_city = st.text_input("City")
            address_province = st.text_input("Province")
            address_kecamatan = st.text_input("Kecamatan")
            phone_number = st.text_input("Phone Number")
            submitted = st.form_submit_button("Create User")
            if submitted:
                payload = {
                    "id_user": id_user,
                    "nama_lengkap": nama_lengkap,
                    "email": email,
                    "domain_email": domain_email,
                    "address": address,
                    "address_zip": address_zip,
                    "address_city": address_city,
                    "address_province": address_province,
                    "address_kecamatan": address_kecamatan,
                    "phone_number": phone_number,
                }
                response = requests.post(f"{BASE_URL}/user/", json=payload)
                st.json(response.json())

    view_user_expander = st.expander("📄 View Users")
    with view_user_expander:
        df = safe_get_dataframe_from_api(f"{BASE_URL}/user/")
        if not df.empty:
            st.dataframe(df)

# Transaction Management Tab
with tab_transaction:
    st.header("💳 Transaction Management")
    create_trx_expander = st.expander("➕ Create Transaction")
    with create_trx_expander:
        with st.form("create_transaction_form"):
            id_transaction = st.text_input("Transaction ID")
            id_user = st.text_input("User ID")
            amount = st.number_input("Amount")
            payment_type = st.text_input("Payment Type")
            shipzip = st.text_input("Shipping Zip")
            shipping_address = st.text_input("Shipping Address")
            shipping_city = st.text_input("Shipping City")
            shipping_province = st.text_input("Shipping Province")
            shipping_kecamatan = st.text_input("Shipping Kecamatan")
            status = st.text_input("Status")
            billing_address = st.text_input("Billing Address")
            billing_city = st.text_input("Billing City")
            billing_province = st.text_input("Billing Province")
            billing_kecamatan = st.text_input("Billing Kecamatan")
            number = st.text_input("Card Number")
            submitted = st.form_submit_button("Create Transaction")
            if submitted:
                payload = {
                    "id_transaction": id_transaction,
                    "id_user": id_user,
                    "amount": amount,
                    "payment_type": payment_type,
                    "shipzip": shipzip,
                    "shipping_address": shipping_address,
                    "shipping_city": shipping_city,
                    "shipping_province": shipping_province,
                    "shipping_kecamatan": shipping_kecamatan,
                    "status": status,
                    "billing_address": billing_address,
                    "billing_city": billing_city,
                    "billing_province": billing_province,
                    "billing_kecamatan": billing_kecamatan,
                    "number": number,
                    "list_of_items": [],
                }
                response = requests.post(f"{BASE_URL}/transaction/", json=payload)
                st.json(response.json())

    view_trx_expander = st.expander("📄 View Transactions")
    with view_trx_expander:
        df = safe_get_dataframe_from_api(f"{BASE_URL}/transaction/")
        if not df.empty:
            st.dataframe(df)

    upload_trx_expander = st.expander("📁 Upload Transaction Batch + Auto Process")
    with upload_trx_expander:
        uploaded_file = st.file_uploader("Upload Transaction JSON File", type=["json"])
        if uploaded_file is not None:
            file_content = uploaded_file.read()
            transactions = json.loads(file_content)

            if isinstance(transactions, list):
                for trx in transactions:
                    res = requests.post(f"{BASE_URL}/transaction/", json=trx)
                    st.success(f"Transaction {trx.get('id_transaction')} created.")
                    process_res = requests.post(
                        f"{BASE_URL}/process/transaction",
                        json={"id_transaction": trx.get("id_transaction")},
                    )
                    if process_res.status_code == 200:
                        risk_data = process_res.json()["data"]
                        st.info(
                            f"Risk Score: {risk_data['risk_score']} | Status: {risk_data['detected_status']}"
                        )
            else:
                st.error("Uploaded JSON must be a list of transactions.")

# Policy Management Tab
with tab_policy:
    st.header("📜 Policy Management")
    upload_policy_expander = st.expander("📁 Upload Policy Batch")
    with upload_policy_expander:
        uploaded_file_policy = st.file_uploader(
            "Upload Policy JSON File", type=["json"]
        )
        if uploaded_file_policy is not None:
            file_content = uploaded_file_policy.read()
            policies = json.loads(file_content)
            if isinstance(policies, list):
                for policy in policies:
                    res = requests.post(f"{BASE_URL}/policy/", json=policy)
                    st.success(f"Policy {policy.get('name')} created.")
            else:
                st.error("Uploaded JSON must be a list of policies.")

# Rule Management Tab
with tab_rule:
    st.header("📏 Rule Management")
    st.info("Untuk sekarang upload dan edit Rules manual via API.")

# Process Transaction Tab
with tab_process:
    st.header("⚙️ Process Transaction Manual")
    id_trx = st.text_input("Transaction ID to Process")
    if st.button("Process Transaction"):
        payload = {"id_transaction": id_trx}
        response = requests.post(f"{BASE_URL}/process/transaction", json=payload)
        st.json(response.json())

# Statistics Tab
with tab_stats:
    st.header("📈 Statistics Visualization")

    stats_type = st.selectbox(
        "Choose Statistics",
        ["Users", "Transactions", "Policies Performance", "Rules Performance"],
    )

    if stats_type == "Users":
        df = safe_get_dataframe_from_api(f"{BASE_URL}/stats/users")
        if not df.empty:
            st.dataframe(df)

    elif stats_type == "Transactions":
        df = safe_get_dataframe_from_api(f"{BASE_URL}/stats/transactions")
        if not df.empty:
            st.subheader("📊 Risk Score Chart")
            st.bar_chart(df.set_index("id_transaction")["risk_score"])

            st.subheader("📄 Export Transaction Risk Report")
            csv = df.to_csv(index=False).encode("utf-8")
            st.download_button(
                label="Download Risk Report as CSV",
                data=csv,
                file_name="risk_report.csv",
                mime="text/csv",
            )

            st.subheader("🥧 Risk Distribution Pie Chart")
            risk_labels = []
            for score in df["risk_score"]:
                if score > 85:
                    risk_labels.append("Fraud")
                elif score >= 40:
                    risk_labels.append("Suspect")
                else:
                    risk_labels.append("Normal")

            pie_df = pd.DataFrame({"Risk Status": risk_labels})
            pie_chart = pie_df["Risk Status"].value_counts().reset_index()
            pie_chart.columns = ["Status", "Count"]

            chart = (
                alt.Chart(pie_chart)
                .mark_arc()
                .encode(
                    theta=alt.Theta(field="Count", type="quantitative"),
                    color=alt.Color(field="Status", type="nominal"),
                    tooltip=["Status", "Count"],
                )
            )
            st.altair_chart(chart, use_container_width=True)

            st.subheader("🚨 High Risk Alerts")
            high_risk = df[df["risk_score"] > 85]
            if not high_risk.empty:
                st.error(f"⚠️ Warning! {len(high_risk)} transactions flagged as FRAUD:")
                st.dataframe(high_risk)
            else:
                st.success("✅ No high risk transactions detected!")

    elif stats_type == "Policies Performance":
        df = safe_get_dataframe_from_api(f"{BASE_URL}/stats/policies-performance")
        if not df.empty:
            st.dataframe(df)

    elif stats_type == "Rules Performance":
        df = safe_get_dataframe_from_api(f"{BASE_URL}/stats/rules-performance")
        if not df.empty:
            st.dataframe(df)

# Monitor Transactions Tab
with tab_monitor:
    st.header("🔄 Realtime Transaction Monitoring")

    refresh_interval = st.slider("Refresh Interval (seconds)", 5, 60, 10)

    run_monitor = st.checkbox("Run Monitor")

    if run_monitor:
        placeholder = st.empty()
        while True:
            with placeholder.container():
                st.write("Refreshing...")
                df = safe_get_dataframe_from_api(f"{BASE_URL}/transaction/")
                if not df.empty:
                    st.dataframe(df[["id_transaction", "amount", "status"]])
                else:
                    st.warning("No transaction data or server error.")
            time.sleep(refresh_interval)
